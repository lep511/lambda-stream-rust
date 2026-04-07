use stream_rust::PromptRequest;

// ─── Deserialización completa ───────────────────────────────────────────────

#[test]
fn deserialize_all_fields() {
    let json =
        r#"{"prompt":"Hola","model_id":"anthropic.claude-sonnet-4-6-v1:0","max_tokens":2048}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.prompt.unwrap(), "Hola");
    assert_eq!(req.model_id, "anthropic.claude-sonnet-4-6-v1:0");
    assert_eq!(req.max_tokens, 2048);
}

// ─── Valores por defecto ────────────────────────────────────────────────────

#[test]
fn default_model_id_is_claude_sonnet() {
    let json = r#"{"prompt":"Test"}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert!(
        req.model_id.contains("anthropic") || req.model_id.contains("claude"),
        "default model_id debería ser Anthropic/Claude, got: {}",
        req.model_id
    );
}

#[test]
fn default_max_tokens_is_1024() {
    let json = r#"{"prompt":"Test","model_id":"anthropic.claude-sonnet-4-6-v1:0"}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.max_tokens, 1024);
}

#[test]
fn explicit_model_overrides_default() {
    let json = r#"{"prompt":"Test","model_id":"anthropic.claude-3-5-sonnet-20240620-v1:0"}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.model_id, "anthropic.claude-3-5-sonnet-20240620-v1:0");
}

#[test]
fn explicit_max_tokens_overrides_default() {
    let json = r#"{"prompt":"Test","max_tokens":4096}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.max_tokens, 4096);
}

// ─── Prompt y messages son opcionales ───────────────────────────────────────

#[test]
fn missing_prompt_succeeds_with_none() {
    let json = r#"{"model_id":"anthropic.claude-sonnet-4-6-v1:0"}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert!(req.prompt.is_none());
}

#[test]
fn null_prompt_succeeds_with_none() {
    let json = r#"{"prompt":null}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert!(req.prompt.is_none());
}

#[test]
fn empty_prompt_succeeds() {
    let json = r#"{"prompt":""}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.prompt.unwrap(), "");
}

// ─── Messages array ────────────────────────────────────────────────────────

#[test]
fn deserialize_with_messages() {
    let json = r#"{"messages":[{"role":"user","content":"Hola"},{"role":"assistant","content":"Hola!"},{"role":"user","content":"Que tal?"}]}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert!(req.prompt.is_none());
    assert_eq!(req.messages.as_ref().unwrap().len(), 3);
}

#[test]
fn deserialize_with_both_prompt_and_messages() {
    let json = r#"{"prompt":"ignored","messages":[{"role":"user","content":"Hola"}]}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert!(req.prompt.is_some());
    assert!(req.messages.is_some());
}

// ─── JSON inválido ──────────────────────────────────────────────────────────

#[test]
fn invalid_json_fails() {
    let result: Result<PromptRequest, _> = serde_json::from_str("not json");
    assert!(result.is_err());
}

#[test]
fn empty_string_fails() {
    let result: Result<PromptRequest, _> = serde_json::from_str("");
    assert!(result.is_err());
}

#[test]
fn array_instead_of_object_fails() {
    let result: Result<PromptRequest, _> = serde_json::from_str("[1,2,3]");
    assert!(result.is_err());
}

// ─── Tipos incorrectos ─────────────────────────────────────────────────────

#[test]
fn prompt_as_number_fails() {
    let json = r#"{"prompt":42}"#;
    let result: Result<PromptRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn max_tokens_as_string_fails() {
    let json = r#"{"prompt":"Test","max_tokens":"not_a_number"}"#;
    let result: Result<PromptRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn max_tokens_as_negative_fails() {
    let json = r#"{"prompt":"Test","max_tokens":-1}"#;
    let result: Result<PromptRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// ─── Campos extra son ignorados ─────────────────────────────────────────────

#[test]
fn extra_fields_are_ignored() {
    let json = r#"{"prompt":"Test","model_id":"anthropic.claude-sonnet-4-6-v1:0","max_tokens":1024,"extra":"ignored","nested":{"a":1}}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.prompt.unwrap(), "Test");
}

// ─── Unicode ────────────────────────────────────────────────────────────────

#[test]
fn unicode_prompt() {
    let json = r#"{"prompt":"こんにちは世界 🌍"}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.prompt.unwrap(), "こんにちは世界 🌍");
}

#[test]
fn escaped_characters_in_prompt() {
    let json = r#"{"prompt":"line1\nline2\ttab"}"#;
    let req: PromptRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.prompt.unwrap(), "line1\nline2\ttab");
}
