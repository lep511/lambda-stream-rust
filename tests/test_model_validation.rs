use stream_rust::is_known_model;

// ─── Modelos válidos (Anthropic/Claude) ─────────────────────────────────────

#[test]
fn anthropic_claude_sonnet_4_6() {
    assert!(is_known_model("anthropic.claude-sonnet-4-6-v1:0"));
}

#[test]
fn anthropic_claude_sonnet_3_5() {
    assert!(is_known_model("anthropic.claude-3-5-sonnet-20240620-v1:0"));
}

#[test]
fn anthropic_claude_haiku() {
    assert!(is_known_model("anthropic.claude-3-haiku-20240307-v1:0"));
}

#[test]
fn anthropic_prefix_only() {
    assert!(is_known_model("anthropic.any-model"));
}

#[test]
fn claude_prefix_only() {
    assert!(is_known_model("claude-v2"));
}

#[test]
fn claude_embedded_in_id() {
    assert!(is_known_model("us.anthropic.claude-sonnet-4-6-v1:0"));
}

// ─── Modelos NO soportados ──────────────────────────────────────────────────

#[test]
fn rejects_empty_string() {
    assert!(!is_known_model(""));
}

#[test]
fn rejects_amazon_nova() {
    assert!(!is_known_model("amazon.nova-micro-v1:0"));
}

#[test]
fn rejects_amazon_titan() {
    assert!(!is_known_model("amazon.titan-text-express-v1"));
}

#[test]
fn rejects_meta_llama() {
    assert!(!is_known_model("meta.llama3-8b-instruct-v1:0"));
}

#[test]
fn rejects_cohere() {
    assert!(!is_known_model("cohere.command-r-plus-v1:0"));
}

#[test]
fn rejects_random_string() {
    assert!(!is_known_model("gpt-4o-mini"));
}

#[test]
fn rejects_partial_nonsense() {
    assert!(!is_known_model("my-custom-model-v1"));
}

// ─── Case sensitivity ───────────────────────────────────────────────────────

#[test]
fn case_sensitive_anthropic() {
    assert!(!is_known_model("Anthropic.model-v2"));
}

#[test]
fn case_sensitive_claude() {
    assert!(!is_known_model("CLAUDE-v2"));
}
