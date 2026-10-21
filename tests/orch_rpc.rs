//! 003 T006: JSON-RPC 2.0 framing and the MCP 2025-11-25 lifecycle (research R-1).

use fallow::orchestration::rpc::{Action, PROTOCOL_VERSION, Server};
use serde_json::{Value, json};

fn reply(a: Action) -> Value {
    match a {
        Action::Reply(line) => {
            assert!(!line.contains('\n'), "one message per line: {line}");
            serde_json::from_str(&line).unwrap()
        }
        other => panic!("expected a reply, got {other:?}"),
    }
}

fn ready() -> Server {
    let mut s = Server::new(json!([{"name": "whoami"}]));
    let r = reply(s.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
    ));
    assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert!(matches!(
        s.handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
        Action::None
    ));
    s
}

#[test]
fn initialize_announces_the_revision_tools_and_server() {
    let mut s = Server::new(json!([]));
    let r = reply(s.handle(
        r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.304"}}}"#,
    ));
    assert_eq!(r["jsonrpc"], "2.0");
    assert_eq!(r["id"], "a", "string ids are kept as given");
    assert_eq!(r["result"]["protocolVersion"], "2025-11-25");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert_eq!(r["result"]["serverInfo"]["name"], "fallow");
    assert_eq!(
        r["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        s.client(),
        Some(("claude-code".to_string(), "2.1.304".to_string()))
    );
}

#[test]
fn requests_before_initialize_are_refused() {
    let mut s = Server::new(json!([]));
    let r = reply(s.handle(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#));
    assert_eq!(r["error"]["code"], -32002);
    assert_eq!(r["id"], 7);
    // ping is allowed at any time.
    let r = reply(s.handle(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#));
    assert_eq!(r["result"], json!({}));
}

#[test]
fn ping_list_and_call() {
    let mut s = ready();
    let r = reply(s.handle(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#));
    assert_eq!(r["result"], json!({}));
    let r = reply(s.handle(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#));
    assert_eq!(r["result"]["tools"][0]["name"], "whoami");
    match s.handle(
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"board_post","arguments":{"text":"hi"},"_meta":{"progressToken":"p1"}}}"#,
    ) {
        Action::Call {
            id,
            name,
            arguments,
            progress,
        } => {
            assert_eq!(id, json!(4));
            assert_eq!(name, "board_post");
            assert_eq!(arguments, json!({"text": "hi"}));
            assert_eq!(progress, Some(json!("p1")));
        }
        other => panic!("{other:?}"),
    }
    // Arguments may be omitted.
    match s.handle(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"whoami"}}"#) {
        Action::Call { arguments, .. } => assert_eq!(arguments, json!({})),
        other => panic!("{other:?}"),
    }
    let r = reply(s.handle(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{}}"#));
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn newer_clients_fall_back_and_unknown_methods_are_errors() {
    let mut s = Server::new(json!([]));
    let r = reply(s.handle(r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#));
    assert_eq!(r["error"]["code"], -32601);
    let mut s = ready();
    let r = reply(s.handle(r#"{"jsonrpc":"2.0","id":9,"method":"resources/list"}"#));
    assert_eq!(r["error"]["code"], -32601);
}

#[test]
fn malformed_input_and_invalid_requests() {
    let mut s = ready();
    let r = reply(s.handle("{not json"));
    assert_eq!(r["error"]["code"], -32700);
    assert_eq!(r["id"], Value::Null);
    let r = reply(s.handle(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#));
    assert_eq!(r["error"]["code"], -32600);
    let r = reply(s.handle(r#"[1,2]"#));
    assert_eq!(r["error"]["code"], -32600);
    let r = reply(s.handle(r#"{"jsonrpc":"2.0","id":1}"#));
    assert_eq!(r["error"]["code"], -32600);
    // Blank lines are ignored.
    assert!(matches!(s.handle("   "), Action::None));
}

#[test]
fn notifications_get_no_reply_and_cancellation_is_surfaced() {
    let mut s = ready();
    assert!(matches!(
        s.handle(r#"{"jsonrpc":"2.0","method":"notifications/roots/list_changed"}"#),
        Action::None
    ));
    match s.handle(
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":4,"reason":"timeout"}}"#,
    ) {
        Action::Cancelled(id) => assert_eq!(id, json!(4)),
        other => panic!("{other:?}"),
    }
    // Replies from the client to requests Fallow never sends are ignored.
    assert!(matches!(
        s.handle(r#"{"jsonrpc":"2.0","id":99,"result":{}}"#),
        Action::None
    ));
}

#[test]
fn reply_builders_produce_one_line_each() {
    use fallow::orchestration::rpc::{error_line, progress_line, result_line};
    let l = result_line(
        &json!(3),
        json!({"content": [{"type": "text", "text": "a\nb"}]}),
    );
    assert!(!l.contains('\n'));
    let v: Value = serde_json::from_str(&l).unwrap();
    assert_eq!(v["result"]["content"][0]["text"], "a\nb");
    let v: Value = serde_json::from_str(&error_line(&json!("x"), -32602, "bad")).unwrap();
    assert_eq!(v["error"]["message"], "bad");
    let v: Value = serde_json::from_str(&progress_line(&json!("p"), 15.0)).unwrap();
    assert_eq!(v["method"], "notifications/progress");
    assert_eq!(v["params"]["progressToken"], "p");
    assert!(v.get("id").is_none());
}
