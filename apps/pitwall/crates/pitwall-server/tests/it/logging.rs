//! The server's own log (ER-27, plan B4a): its lines go to a dated paddock-log file in the log folder, and a playground chat
//! leaves its `pitwall_chat` lines there (start, end, the cause of a failure), with names, counts and times and no content.

use crate::common::harness;
use serde_json::json;
use std::time::Duration;

fn chat_body(marker: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "modelId": "tiny-q4_k_m",
        "messages": [{"role": "user", "content": marker}],
        "temperature": 0.2,
        "maxTokens": 16,
    }))
    .unwrap()
}

async fn chat(h: &crate::common::Harness, body: Vec<u8>) -> String {
    let auth = format!("Bearer {}", h.admin);
    let r = h
        .client
        .send_with(
            "POST",
            "/mgmt/v1/chat",
            Some(body),
            &[("authorization", auth.as_str())],
            Duration::from_secs(20),
        )
        .await
        .unwrap();
    String::from_utf8_lossy(&r.bytes(1 << 20).await.unwrap()).into_owned()
}

#[tokio::test]
async fn a_playground_chat_leaves_its_lines_in_the_server_log_file_without_content() {
    let logs = tempfile::tempdir().unwrap();
    let marker = "planted-marker-4f9a";
    let handle = pitwall_server::logging::install_in(logs.path(), &logs.path().join("crash")).expect("the log installs");
    let h = harness(false).await;

    let answer = chat(&h, chat_body(marker)).await;
    assert!(answer.contains("\"kind\":\"done\""), "{answer}");
    let unknown = chat(
        &h,
        serde_json::to_vec(
            &json!({"modelId": "no-such-model", "messages": [{"role": "user", "content": marker}], "temperature": 0.2, "maxTokens": 16}),
        )
        .unwrap(),
    )
    .await;
    assert!(unknown.contains("\"kind\":\"error\""), "{unknown}");

    let file = handle.active_file().expect("a log file");
    assert!(file.starts_with(logs.path()), "{file:?}");
    assert!(
        file.file_name().unwrap().to_string_lossy().starts_with("pitwall-server."),
        "{file:?}"
    );
    let text = std::fs::read_to_string(&file).unwrap();
    let chat_lines: Vec<&str> = text.lines().filter(|l| l.contains("pitwall_chat")).collect();
    assert!(
        chat_lines.iter().any(|l| l.contains("chat started") && l.contains("tiny-q4_k_m")),
        "{chat_lines:?}"
    );
    assert!(
        chat_lines
            .iter()
            .any(|l| l.contains("chat ended") && l.contains("finish") && l.contains("tokens") && l.contains("ttft")),
        "{chat_lines:?}"
    );
    assert!(
        chat_lines.iter().any(|l| l.contains("chat failed") && l.contains("no-such-model")),
        "{chat_lines:?}"
    );
    assert!(!text.contains(marker), "no content in the log");
    assert!(
        text.contains("\"session\"") || text.contains("@t"),
        "it is the family's line format: {}",
        text.lines().next().unwrap_or("")
    );
}
