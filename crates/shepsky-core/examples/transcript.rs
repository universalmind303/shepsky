//! Print an agent transcript as chat items, one per line, to check the
//! parsers against a real file (format drift shows up here first).
//!
//!   cargo run -p shepsky-core --features transcript --example transcript -- claude <file.jsonl>

use shepsky_core::protocol::ChatBody;
use shepsky_core::transcript::{Ctx, Format, Window, parse_line};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(format), Some(path)) = (args.get(1).and_then(|k| Format::for_kind(k)), args.get(2)) else {
        eprintln!("usage: transcript <claude|codex|pi> <file.jsonl>");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(path).expect("read transcript");
    let home = std::env::var("HOME").ok();
    let ctx = Ctx { cwd: None, home: home.as_deref() };
    let mut entries = Vec::new();
    let mut offset = 0u64;
    for line in text.split_inclusive('\n') {
        entries.extend(parse_line(format, offset, line, &ctx));
        offset += line.len() as u64;
    }
    for item in Window::default().add(entries) {
        let line = match &item.body {
            ChatBody::Prompt { text } => format!("> {text}"),
            ChatBody::Reply { text } => text.clone(),
            ChatBody::Thinking { text } => format!("∴ {text}"),
            ChatBody::Tool { name, summary, status, .. } => format!("● {name} {summary} [{status:?}]"),
            ChatBody::Question { questions, answer } => format!("? {:?} => {answer:?}", questions.first().map(|q| &q.question)),
            ChatBody::Notice { text } => format!("── {text}"),
        };
        println!("{}", line.replace('\n', " ⏎ ").chars().take(140).collect::<String>());
    }
}
