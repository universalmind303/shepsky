//! JNI surface for the Android app. Deliberately tiny — three functions and
//! one callback, all payloads UTF-8 JSON as `byte[]` (sidesteps JNI's
//! modified-UTF-8).
//!
//! Kotlin side: `dev.collie.core.CollieCore`
//!   external fun nativeStart(config: ByteArray, listener: EventListener): Long
//!   external fun nativeSend(handle: Long, msg: ByteArray)
//!   external fun nativeStop(handle: Long)
//! with `interface EventListener { fun onEvent(json: ByteArray) }`.
//!
//! Threading: the tokio runtime produces events into an unbounded channel; one
//! dedicated OS thread stays attached to the JVM and delivers them to the
//! listener in order.

mod client;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};

use jni::JNIEnv;
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JValue};
use jni::sys::jlong;
use tokio::sync::mpsc;

struct Session {
    commands: mpsc::Sender<String>,
    runtime: Option<tokio::runtime::Runtime>,
}

static SESSIONS: OnceLock<Mutex<HashMap<u64, Session>>> = OnceLock::new();
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static INIT: Once = Once::new();

fn sessions() -> &'static Mutex<HashMap<u64, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn init_once() {
    INIT.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("collie"),
        );
        // Pin rustls to the ring provider (the only one we compile).
        let _ = rustls::crypto::ring::default_provider().install_default();
        log_panics();
    });
}

/// Panics in Rust threads otherwise vanish; route them to logcat.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("rust panic: {info}");
        default(info);
    }));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_collie_core_CollieCore_nativeStart(
    env: JNIEnv,
    _class: JClass,
    config: JByteArray,
    listener: JObject,
) -> jlong {
    init_once();

    let config_bytes = match env.convert_byte_array(&config) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::error!("nativeStart: bad config array: {err}");
            return 0;
        }
    };
    let config: client::Config = match serde_json::from_slice(&config_bytes) {
        Ok(config) => config,
        Err(err) => {
            log::error!("nativeStart: bad config json: {err}");
            return 0;
        }
    };
    let (Ok(vm), Ok(listener)) = (env.get_java_vm(), env.new_global_ref(&listener)) else {
        log::error!("nativeStart: could not grab JavaVM/listener");
        return 0;
    };

    let (events_tx, events_rx) = std::sync::mpsc::channel::<String>();
    let (commands_tx, commands_rx) = mpsc::channel::<String>(64);

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("collie-net")
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            log::error!("nativeStart: tokio runtime: {err}");
            return 0;
        }
    };
    runtime.spawn(client::run(config, commands_rx, events_tx));

    // Dedicated delivery thread: attached once, detached when the channel
    // closes (runtime drop on stop).
    std::thread::Builder::new()
        .name("collie-events".into())
        .spawn(move || deliver_events(vm, listener, events_rx))
        .expect("spawn event thread");

    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    sessions().lock().unwrap().insert(
        handle,
        Session {
            commands: commands_tx,
            runtime: Some(runtime),
        },
    );
    log::info!("session {handle} started");
    handle as jlong
}

fn deliver_events(vm: jni::JavaVM, listener: GlobalRef, events: std::sync::mpsc::Receiver<String>) {
    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(err) => {
            log::error!("event thread could not attach to JVM: {err}");
            return;
        }
    };
    while let Ok(event) = events.recv() {
        let Ok(array) = env.byte_array_from_slice(event.as_bytes()) else {
            continue;
        };
        let result = env.call_method(
            listener.as_obj(),
            "onEvent",
            "([B)V",
            &[JValue::Object(array.as_ref())],
        );
        if result.is_err() {
            // A throwing listener must not kill delivery (or the process).
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
        }
        // Local refs pile up in long-lived attached threads; drop eagerly.
        let _ = env.delete_local_ref(array);
    }
    log::info!("event thread exiting");
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_collie_core_CollieCore_nativeSend(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    msg: JByteArray,
) {
    let Ok(bytes) = env.convert_byte_array(&msg) else { return };
    let Ok(text) = String::from_utf8(bytes) else { return };
    let sender = {
        let sessions = sessions().lock().unwrap();
        sessions.get(&(handle as u64)).map(|s| s.commands.clone())
    };
    match sender {
        Some(sender) => {
            if sender.try_send(text).is_err() {
                log::warn!("command dropped (queue full or session stopping)");
            }
        }
        None => log::warn!("nativeSend: unknown handle {handle}"),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_collie_core_CollieCore_nativeStop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    let session = sessions().lock().unwrap().remove(&(handle as u64));
    if let Some(mut session) = session {
        if let Some(runtime) = session.runtime.take() {
            // Don't block a JVM thread on shutdown.
            runtime.shutdown_background();
        }
        log::info!("session {handle} stopped");
    }
}
