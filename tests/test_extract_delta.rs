use stream_rust::extract_text_delta;

// ─── Anthropic Claude ───────────────────────────────────────────────────────

#[test]
fn claude_text_delta() {
    let payload = br#"{"delta":{"type":"text_delta","text":"Bonjour le monde"}}"#;
    assert_eq!(extract_text_delta(payload), Some("Bonjour le monde".into()));
}

#[test]
fn claude_empty_text() {
    let payload = br#"{"delta":{"type":"text_delta","text":""}}"#;
    assert_eq!(extract_text_delta(payload), Some(String::new()));
}

#[test]
fn claude_with_extra_fields() {
    let payload =
        br#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#;
    assert_eq!(extract_text_delta(payload), Some("ok".into()));
}

#[test]
fn claude_unicode() {
    let payload = r#"{"delta":{"text":"日本語テスト 🦀"}}"#;
    assert_eq!(
        extract_text_delta(payload.as_bytes()),
        Some("日本語テスト 🦀".into())
    );
}

#[test]
fn claude_multiline() {
    let payload = r#"{"delta":{"text":"línea1\nlínea2"}}"#;
    assert_eq!(
        extract_text_delta(payload.as_bytes()),
        Some("línea1\nlínea2".into())
    );
}

#[test]
fn claude_long_text() {
    let long = "a".repeat(10_000);
    let payload = format!(r#"{{"delta":{{"text":"{long}"}}}}"#);
    assert_eq!(extract_text_delta(payload.as_bytes()), Some(long));
}

#[test]
fn claude_special_characters() {
    let payload = br#"{"delta":{"text":"quotes: \"hello\" and backslash: \\"}}"#;
    assert_eq!(
        extract_text_delta(payload),
        Some(r#"quotes: "hello" and backslash: \"#.into())
    );
}

#[test]
fn claude_whitespace_only() {
    let payload = br#"{"delta":{"text":"   \t\n  "}}"#;
    assert_eq!(extract_text_delta(payload), Some("   \t\n  ".into()));
}

// ─── Casos negativos ────────────────────────────────────────────────────────

#[test]
fn empty_payload_returns_none() {
    assert_eq!(extract_text_delta(b""), None);
}

#[test]
fn invalid_json_returns_none() {
    assert_eq!(extract_text_delta(b"not json at all"), None);
}

#[test]
fn empty_json_object_returns_none() {
    assert_eq!(extract_text_delta(b"{}"), None);
}

#[test]
fn null_value_returns_none() {
    assert_eq!(extract_text_delta(b"null"), None);
}

#[test]
fn wrong_structure_returns_none() {
    let payload = br#"{"foo":"bar","baz":123}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn delta_without_text_returns_none() {
    let payload = br#"{"delta":{"type":"text_delta"}}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn delta_text_as_number_returns_none() {
    let payload = br#"{"delta":{"text":123}}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn delta_text_as_null_returns_none() {
    let payload = br#"{"delta":{"text":null}}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn delta_text_as_array_returns_none() {
    let payload = br#"{"delta":{"text":["a","b"]}}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn delta_text_as_object_returns_none() {
    let payload = br#"{"delta":{"text":{"nested":"value"}}}"#;
    assert_eq!(extract_text_delta(payload), None);
}

// ─── Eventos de stream que no son text_delta ────────────────────────────────

#[test]
fn message_start_event_returns_none() {
    let payload = br#"{"type":"message_start","message":{"id":"msg_01","role":"assistant"}}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn content_block_start_returns_none() {
    let payload =
        br#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn message_stop_event_returns_none() {
    let payload = br#"{"type":"message_stop"}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn message_delta_stop_reason_returns_none() {
    let payload = br#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#;
    assert_eq!(extract_text_delta(payload), None);
}

#[test]
fn ping_event_returns_none() {
    let payload = br#"{"type":"ping"}"#;
    assert_eq!(extract_text_delta(payload), None);
}
