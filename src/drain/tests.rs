use super::*;
use crate::events::EventLog;

/// `drain_chat` OK append une ligne `assistant_msg` au journal.
#[test]
fn drain_chat_ok_emits_assistant_msg() {
    let path = std::env::temp_dir().join(format!("crypte-drain-test-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut app = App::offline().unwrap();
    app.events = EventLog::new(Some(path.clone()));
    let done = Completion {
        text: String::from("boo"),
        tool_calls: Vec::new(),
        prompt_tokens: 3,
        completion_tokens: 5,
    };
    app.drain_chat(0, 0, Ok(done));
    let content = std::fs::read_to_string(&path).unwrap();
    let line = content.lines().next().unwrap();
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(v["type"], "assistant_msg");
    assert_eq!(v["chat"], 1);
    assert_eq!(v["text"], "boo");
    assert_eq!(v["completion_tokens"], 5);
    let _ = std::fs::remove_file(&path);
}
