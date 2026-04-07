use serde_json::json;
use stream_rust::{PromptRequest, build_model_body};

fn make_request(prompt: &str, model_id: &str, max_tokens: u32) -> PromptRequest {
    PromptRequest {
        prompt: Some(prompt.to_string()),
        messages: None,
        model_id: model_id.to_string(),
        max_tokens,
    }
}

// ─── Estructura del body Claude (Messages API) ─────────────────────────────

#[test]
fn claude_body_has_anthropic_version() {
    let req = make_request("Hola", "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();

    assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
}

#[test]
fn claude_body_has_max_tokens() {
    let req = make_request("Hola", "anthropic.claude-sonnet-4-6-v1:0", 512);
    let body = build_model_body(&req).unwrap();

    assert_eq!(body["max_tokens"], 512);
}

#[test]
fn claude_body_has_messages_array() {
    let req = make_request("Hola", "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();

    assert!(body["messages"].is_array());
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn claude_body_message_has_user_role() {
    let req = make_request("Hola", "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();

    assert_eq!(body["messages"][0]["role"], "user");
}

#[test]
fn claude_body_message_has_prompt_as_content() {
    let req = make_request("Explica Rust", "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();

    assert_eq!(body["messages"][0]["content"], "Explica Rust");
}

// ─── Snapshot completo ──────────────────────────────────────────────────────

#[test]
fn full_body_snapshot() {
    let req = make_request("¿Qué es Rust?", "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();
    let expected = json!({
        "anthropic_version": "bedrock-2023-05-31",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "¿Qué es Rust?"}
        ]
    });
    assert_eq!(body, expected);
}

// ─── Preservación del prompt ────────────────────────────────────────────────

#[test]
fn preserves_unicode_prompt() {
    let prompt = "Explica con acentos: ñ, ü, é y emojis 🎉";
    let req = make_request(prompt, "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();

    assert_eq!(body["messages"][0]["content"], prompt);
}

#[test]
fn preserves_empty_prompt() {
    let req = make_request("", "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();

    assert_eq!(body["messages"][0]["content"], "");
}

#[test]
fn preserves_long_prompt() {
    let long_text = "a".repeat(10_000);
    let req = make_request(&long_text, "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();

    assert_eq!(
        body["messages"][0]["content"].as_str().unwrap().len(),
        10_000
    );
}

#[test]
fn preserves_multiline_prompt() {
    let prompt = "línea 1\nlínea 2\nlínea 3";
    let req = make_request(prompt, "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();

    assert_eq!(body["messages"][0]["content"], prompt);
}

// ─── Valores extremos de max_tokens ─────────────────────────────────────────

#[test]
fn zero_max_tokens() {
    let req = make_request("Test", "anthropic.claude-sonnet-4-6-v1:0", 0);
    let body = build_model_body(&req).unwrap();
    assert_eq!(body["max_tokens"], 0);
}

#[test]
fn large_max_tokens() {
    let req = make_request("Test", "anthropic.claude-sonnet-4-6-v1:0", 100_000);
    let body = build_model_body(&req).unwrap();
    assert_eq!(body["max_tokens"], 100_000);
}

// ─── Serialización ──────────────────────────────────────────────────────────

#[test]
fn body_is_serializable() {
    let req = make_request("test", "anthropic.claude-sonnet-4-6-v1:0", 1024);
    let body = build_model_body(&req).unwrap();
    let bytes = serde_json::to_vec(&body);
    assert!(bytes.is_ok());
}

#[test]
fn body_roundtrips_through_json() {
    let req = make_request("test prompt", "anthropic.claude-sonnet-4-6-v1:0", 2048);
    let body = build_model_body(&req).unwrap();
    let serialized = serde_json::to_string(&body).unwrap();
    let deserialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(body, deserialized);
}

// ─── Cualquier model_id produce el mismo formato Claude ─────────────────────

#[test]
fn any_model_id_produces_claude_format() {
    let models = [
        "anthropic.claude-sonnet-4-6-v1:0",
        "unknown-model",
        "anything",
        "",
    ];
    for model in models {
        let req = make_request("test", model, 1024);
        let body = build_model_body(&req).unwrap();
        assert_eq!(
            body["anthropic_version"], "bedrock-2023-05-31",
            "model_id={model} should produce Claude format"
        );
    }
}

// ─── Messages array ─────────────────────────────────────────────────────────

#[test]
fn messages_array_takes_priority() {
    let req = PromptRequest {
        prompt: Some("ignored".to_string()),
        messages: Some(vec![
            json!({"role": "user", "content": "Hola"}),
            json!({"role": "assistant", "content": "Hola!"}),
            json!({"role": "user", "content": "Como estas?"}),
        ]),
        model_id: "anthropic.claude-sonnet-4-6-v1:0".to_string(),
        max_tokens: 1024,
    };
    let body = build_model_body(&req).unwrap();
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    assert_eq!(body["messages"][2]["content"], "Como estas?");
}

#[test]
fn no_prompt_no_messages_returns_none() {
    let req = PromptRequest {
        prompt: None,
        messages: None,
        model_id: "anthropic.claude-sonnet-4-6-v1:0".to_string(),
        max_tokens: 1024,
    };
    assert!(build_model_body(&req).is_none());
}
