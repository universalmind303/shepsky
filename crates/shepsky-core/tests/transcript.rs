//! Each agent's transcript format, read from a synthetic fixture.
#![cfg(feature = "transcript")]

use shepsky_core::protocol::{ChatBody, ChatItem, ToolStatus};
use shepsky_core::transcript::{Ctx, Format, Window, parse_line};

const CTX: Ctx = Ctx { cwd: Some("/Users/demo/src/todo-app"), home: Some("/Users/demo") };

fn read(format: Format, file: &str) -> Vec<ChatItem> {
    let text = std::fs::read_to_string(format!("{}/tests/fixtures/transcripts/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut window = Window::default();
    let mut offset = 0u64;
    let mut entries = Vec::new();
    for line in text.split_inclusive('\n') {
        entries.extend(parse_line(format, offset, line, &CTX));
        offset += line.len() as u64;
    }
    window.add(entries)
}

/// One line per item: `kind: text`.
fn lines(items: &[ChatItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match &item.body {
            ChatBody::Prompt { text } => format!("prompt: {text}"),
            ChatBody::Reply { text } => format!("reply: {text}"),
            ChatBody::Thinking { text } => format!("thinking: {text}"),
            ChatBody::Tool { name, summary, status, output, .. } => {
                let status = match status {
                    ToolStatus::Running => "running",
                    ToolStatus::Ok => "ok",
                    ToolStatus::Error => "error",
                };
                format!("tool {name} [{status}]: {summary} => {}", output.as_deref().unwrap_or("-"))
            }
            ChatBody::Question { questions, answer } => {
                format!("question: {} {:?} => {}", questions[0].question, questions[0].options, answer.as_deref().unwrap_or("-"))
            }
            ChatBody::Notice { text } => format!("notice: {text}"),
        })
        .collect()
}

#[test]
fn claude() {
    let items = read(Format::Claude, "claude.jsonl");
    assert_eq!(
        lines(&items),
        [
            "notice: /model opus",
            "prompt: Fix the flaky date test",
            "thinking: ",
            "reply: Let me look at the test first.",
            "tool Read [ok]: src/date.test.ts => 1\timport { parse } from './date'\n2\ttest('parses', () => {})",
            "tool Edit [ok]: src/date.ts => The file has been updated.",
            "tool Bash [error]: $ npm test -- date => FAIL src/date.test.ts\n1 failed",
            "question: Pin the timezone or mock the clock? [\"Pin UTC\", \"Mock clock\"] => User has answered your questions: \"Pin the timezone or mock the clock?\"=\"Pin UTC\".",
            "prompt: also update the changelog",
            "notice: interrupted",
            "notice: context compacted",
            "reply: Done: the test pins **UTC** now.",
        ]
    );
    // Ids follow the file, so the phone can order and page by them.
    assert!(items.windows(2).all(|w| w[0].id < w[1].id));
    let edit = items.iter().find_map(|i| match &i.body {
        ChatBody::Tool { name, detail, .. } if name == "Edit" => detail.clone(),
        _ => None,
    });
    assert_eq!(edit.as_deref(), Some("- new Date()\n+ new Date(Date.UTC(2026, 0, 1))"));
}

#[test]
fn codex() {
    assert_eq!(
        lines(&read(Format::Codex, "codex.jsonl")),
        [
            "prompt: Add rate limiting to /v1/ingest",
            "thinking: **Planning the limiter**",
            "tool shell [ok]: $ rg -n ingest src => src/routes.ts:12: ingest",
            "tool edit [ok]: src/limit.ts => -",
            "tool shell [error]: $ npm test => 1 failing",
            "tool github.get_pr [ok]: pr_number: 7 => PR #7: open",
            "reply: Added a limiter; one test still fails.",
            "notice: interrupted",
            "notice: context compacted",
        ]
    );
}

#[test]
fn pi() {
    assert_eq!(
        lines(&read(Format::Pi, "pi.jsonl")),
        [
            "prompt: Rename the settings page",
            "thinking: Find the page first.",
            "tool bash [ok]: $ ls src/pages => settings.tsx",
            "tool edit [error]: src/pages/settings.tsx => no match",
            "reply: The page is now called Preferences.",
        ]
    );
}

#[test]
fn any_slice_parses_on_its_own() {
    // Reading only the second half (paging backwards starts mid-file) must
    // still produce items: no line depends on an earlier one to parse.
    let text = std::fs::read_to_string(format!("{}/tests/fixtures/transcripts/claude.jsonl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let half: Vec<&str> = text.split_inclusive('\n').collect();
    let tail = &half[half.len() / 2..];
    let n: usize = tail.iter().map(|l| parse_line(Format::Claude, 0, l, &CTX).len()).sum();
    assert!(n > 0);
}
