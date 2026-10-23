use super::*;
use shepsky_core::herdr::Agent;
use shepsky_core::store::Store;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UnixListener};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message as ClientFrame};

type Phone = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn receive(phone: &mut Phone) -> serde_json::Value {
    let frame = tokio::time::timeout(Duration::from_secs(2), phone.next()).await.unwrap().unwrap().unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

async fn send(phone: &mut Phone, value: serde_json::Value) {
    phone.send(ClientFrame::Text(value.to_string().into())).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_security_boundaries() {
    let mut store = Store::default();
    store.pane_ids.insert("w1:p1".into());
    store.agents.insert("w1:p1".into(), Agent { pane_id: "w1:p1".into(), ..Agent::default() });
    let (_ui_tx, ui_rx) = watch::channel(UiState { store: Arc::new(store), ..UiState::default() });
    let (notes_tx, _) = broadcast::channel(1);
    let home = std::env::temp_dir();
    let socket_path = home.join(format!("security-{}.sock", std::process::id()));
    let mock = UnixListener::bind(&socket_path).unwrap();
    let client = HerdrClient::new(socket_path.clone());
    let state = Arc::new(ServeState {
        ui_rx, notes_tx, hub: TerminalHub::new(client.clone()), client: client.clone(),
        token: "synthetic-api-token".into(), demo_input: false, demo_agent: None,
        home: home.clone(), chat_places: chat::Places { home, demo: None },
    });
    let app = Router::new().route("/ws", get(ws_handler)).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    for value in [json!({"type":"auth","token":"wrong"}), json!({"type":"prompt","pane_id":"w1:p1","text":"test"}), json!({"type":"auth","token":"x".repeat(MAX_AUTH_BYTES + 1)})] {
        let (mut phone, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        send(&mut phone, value).await;
        assert_eq!(receive(&mut phone).await["code"], "unauthorized");
    }

    let (mut phone, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    send(&mut phone, json!({"type":"auth","token":"synthetic-api-token"})).await;
    let hello = receive(&mut phone).await;
    assert_eq!(hello["type"], "hello");
    assert!(hello.get("start_kinds").is_none());
    assert_eq!(receive(&mut phone).await["type"], "state");
    for request in [
        json!({"type":"start_agent","kind":"codex","cwd":"~","op_id":"start"}),
        json!({"type":"rename_agent","pane_id":"w1:p1","name":"safe","op_id":"rename"}),
        json!({"type":"close_agent","pane_id":"w1:p1","op_id":"close"}),
    ] {
        let op_id = request["op_id"].clone();
        send(&mut phone, request).await;
        let response = receive(&mut phone).await;
        assert_eq!(response["code"], "operation_unavailable");
        assert_eq!(response["op_id"], op_id);
    }
    send(&mut phone, json!({"type":"rpc","method":"pane.send_text"})).await;
    assert_eq!(receive(&mut phone).await["code"], "bad_message");
    send(&mut phone, json!({"type":"prompt","pane_id":"w1:p1","text":"test","op_id":"x".repeat(129)})).await;
    assert_eq!(receive(&mut phone).await["code"], "too_large");
    assert!(tokio::time::timeout(Duration::from_millis(30), mock.accept()).await.is_err());

    let responder = tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = mock.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "agent.prompt");
            assert!(tokio::time::timeout(Duration::from_millis(30), mock.accept()).await.is_err());
            let reply = format!("{}\n", json!({"id":request["id"],"result":{}}));
            stream.get_mut().write_all(reply.as_bytes()).await.unwrap();
        }
    });
    for n in 0..2 {
        send(&mut phone, json!({"type":"prompt","pane_id":"w1:p1","text":"synthetic","op_id":n.to_string()})).await;
    }
    for n in 0..2 {
        let response = receive(&mut phone).await;
        assert_eq!(response["type"], "ack");
        assert_eq!(response["op_id"], n.to_string());
    }
    responder.await.unwrap();

    let mut held = Vec::new();
    for _ in 1..MAX_CONNECTIONS {
        held.push(tokio_tungstenite::connect_async(&url).await.unwrap().0);
    }
    let rejected = tokio_tungstenite::connect_async(&url).await.unwrap_err();
    assert!(matches!(rejected, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == StatusCode::SERVICE_UNAVAILABLE));
    drop(held);
    phone.close(None).await.unwrap();
    drop(phone);
    for _ in 0..100 {
        if CONNECTIONS.available_permits() == MAX_CONNECTIONS { break }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(CONNECTIONS.available_permits(), MAX_CONNECTIONS);

    let (mut oversized, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let _ = oversized.send(ClientFrame::Text("x".repeat(MAX_MESSAGE_BYTES + 1).into())).await;
    let ended = tokio::time::timeout(Duration::from_secs(2), oversized.next()).await.unwrap();
    assert!(!matches!(ended, Some(Ok(ClientFrame::Text(_)))));

    for method in ["pane.send_text", "pane.send_keys", "pane.close", "pane.rename", "agent.rename", "agent.start", "workspace.create", "tab.create"] {
        assert!(client.call(method, json!({})).await.unwrap_err().to_string().contains("allowlist"));
    }
    assert!(log::STATIC_MAX_LEVEL <= log::LevelFilter::Info);
    let prompt = ClientMsg::Prompt { pane_id: "w1:p1".into(), text: "test".into(), op_id: None };
    assert!(unsafe_operation(&prompt, true).is_some());
    assert!(unsafe_operation(&prompt, false).is_none());
    server.abort();
    std::fs::remove_file(socket_path).unwrap();
}
