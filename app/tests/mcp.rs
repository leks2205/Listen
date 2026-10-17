//! The MCP server's tools against the demo app, and one call over HTTP.

use listen::Args;
use listen::app::mcp::McpMsg;
use listen::app::{Listen, Message};
use listen::mcp::{self, Call, Reply};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn demo() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    let _ = s.update(Message::PlayDemoAlbum(0));
    s
}

/// Calls a tool that answers right away.
fn call(s: &mut Listen, tool: &str, args: Value) -> Result<Value, String> {
    let (reply, mut answer) = Reply::new();
    let _ = s.update(Message::Mcp(McpMsg::Call(Call { tool: tool.into(), args, reply })));
    answer.try_recv().expect("answered at once")
}

#[test]
fn playback_and_queue_tools() {
    let mut s = demo();
    let now = call(&mut s, "get_now_playing", json!({})).unwrap();
    let title = now["song"]["title"].as_str().expect("a song").to_string();
    assert_eq!(s.queue.current().unwrap().song.title, title);
    assert!(now["song"]["ref"].as_str().unwrap().starts_with("song:"));

    let queue = call(&mut s, "get_queue", json!({})).unwrap();
    let entries = queue["entries"].as_array().unwrap();
    let before = entries.len();
    assert!(before > 1);
    assert_eq!(entries.iter().filter(|e| e["current"] == true).count(), 1);

    // A ref from the queue goes to its end again.
    let last = entries.last().unwrap()["ref"].as_str().unwrap().to_string();
    let r = call(&mut s, "add_to_queue", json!({ "refs": [last] })).unwrap();
    assert_eq!(r["items"], 1);
    assert_eq!(s.queue.len(), before + 1);

    // Unknown refs and tools are refused with a reason.
    let e = call(&mut s, "add_to_queue", json!({ "refs": ["song:x|nope"] })).unwrap_err();
    assert!(e.contains("Unknown ref"), "{e}");

    let r = call(&mut s, "set_repeat", json!({ "repeat": "one" })).unwrap();
    assert_eq!(r["repeat"], "one");
    assert_eq!(call(&mut s, "get_now_playing", json!({})).unwrap()["repeat"], "one");

    let uid = s.queue.iter().last().unwrap().uid;
    let r = call(&mut s, "remove_from_queue", json!({ "uids": [uid] })).unwrap();
    assert_eq!(r["removed"], 1);
    assert_eq!(s.queue.len(), before);
}

#[test]
fn ai_dj_and_settings_tools() {
    let mut s = demo();
    let r =
        call(&mut s, "set_ai_dj", json!({ "language": "fr", "location": "Kiel", "details": "Call me Sam." }))
            .unwrap();
    assert_eq!(r["language"], "fr");
    assert_eq!(r["language_spoken"], "French");
    assert_eq!(s.config.settings.ai_dj.location, "Kiel");
    assert_eq!(s.config.settings.ai_dj.notes, "Call me Sam.");
    assert!(call(&mut s, "set_ai_dj", json!({ "language": "xx" })).is_err());
    assert!(call(&mut s, "set_ai_dj", json!({ "persona": "nobody" })).is_err());
    // Switching on needs a key; the key is never shown.
    assert!(call(&mut s, "set_ai_dj", json!({ "enabled": true })).unwrap_err().contains("Gemini key"));
    let personas = call(&mut s, "list_personas", json!({})).unwrap();
    assert!(personas["personas"].as_array().unwrap().len() >= 4);

    let r = call(
        &mut s,
        "set_settings",
        json!({ "settings": { "history": { "enabled": false }, "playback": { "gapless": 7 }, "nonsense": 1 } }),
    )
    .unwrap();
    assert!(!s.config.settings.history.enabled);
    let refused: Vec<String> =
        r["refused"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert!(refused.iter().any(|i| i.starts_with("playback.gapless")), "{refused:?}");
    assert!(refused.iter().any(|i| i.starts_with("nonsense")), "{refused:?}");

    let all = call(&mut s, "get_settings", json!({})).unwrap();
    assert_eq!(all["history"]["enabled"], false);
    assert!(all["ai_dj"].is_object());
    let text = all.to_string();
    assert!(!text.contains("password") && !text.to_lowercase().contains("api_key\":\""), "no secrets");
}

#[test]
fn a_tool_call_over_http_reaches_the_app() {
    let mut s = demo();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().worker_threads(2).build().unwrap();
    let cfg = mcp::Config { bind: "127.0.0.1".into(), port: 0, token: "t0ken".into() };
    let running = rt.block_on(mcp::start(cfg, s.mcp.sender())).expect("starts");
    let addr = running.addr;
    let client = rt.spawn(async move {
        let body = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": "set_volume", "arguments": { "volume": 35 } } })
        .to_string();
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer t0ken\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        c.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).await.unwrap();
        out
    });
    // The app's side: what the subscription would deliver.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !client.is_finished() && std::time::Instant::now() < deadline {
        if let Some(c) = s.mcp.poll_call() {
            let _ = s.update(Message::Mcp(McpMsg::Call(c)));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = rt.block_on(client).unwrap();
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
    let body: Value = serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["id"], 7);
    assert_eq!(body["result"]["isError"], false, "{body}");
    assert_eq!(s.playback.volume, 35);
    drop(running);
}
