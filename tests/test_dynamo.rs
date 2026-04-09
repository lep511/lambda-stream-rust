/// Tests para el módulo dynamo: funciones puras (sin I/O).
use serde_json::json;
use stream_rust::dynamo::{ChatHistory, ChatMessage, ChatMetadata, epoch_to_iso8601};

// ─── Tests de build_system_prompt ──────────────────────────────────────────

#[test]
fn test_system_prompt_with_metadata_and_language() {
    let meta = Some(ChatMetadata {
        chat_id: 123,
        last_update_id: 100,
        last_message_at: "2026-04-08T10:00:00Z".to_string(),
        first_name: "Esteban".to_string(),
        language_code: Some("es".to_string()),
        message_count: 37,
        last_model: "claude-sonnet-4-6".to_string(),
        updated_at: "2026-04-08T10:00:00Z".to_string(),
    });

    let prompt = ChatHistory::build_system_prompt(&meta, "Esteban", &Some("es".to_string()));

    assert!(prompt.contains("Esteban"), "debe incluir el nombre del usuario");
    assert!(prompt.contains("'es'"), "debe incluir el idioma");
    assert!(prompt.contains("37 messages"), "debe incluir el conteo de mensajes");
}

#[test]
fn test_system_prompt_without_metadata() {
    let prompt = ChatHistory::build_system_prompt(&None, "Ana", &None);

    assert!(prompt.contains("Ana"), "debe incluir el nombre");
    assert!(!prompt.contains("language"), "no debe mencionar idioma");
    assert!(!prompt.contains("messages with"), "no debe mencionar conteo");
}

#[test]
fn test_system_prompt_with_metadata_no_language() {
    let meta = Some(ChatMetadata {
        chat_id: 456,
        last_update_id: 50,
        last_message_at: "2026-04-08T10:00:00Z".to_string(),
        first_name: "Carlos".to_string(),
        language_code: None,
        message_count: 5,
        last_model: "claude-sonnet-4-6".to_string(),
        updated_at: "2026-04-08T10:00:00Z".to_string(),
    });

    let prompt = ChatHistory::build_system_prompt(&meta, "Carlos", &None);

    assert!(prompt.contains("Carlos"));
    assert!(!prompt.contains("language"));
    assert!(prompt.contains("5 messages"));
}

#[test]
fn test_system_prompt_empty_name() {
    let prompt = ChatHistory::build_system_prompt(&None, "", &None);

    assert!(!prompt.contains("name is ."), "no debe agregar nombre vacío");
}

// ─── Tests de build_bedrock_messages ───────────────────────────────────────

#[test]
fn test_bedrock_messages_empty_history() {
    let messages = ChatHistory::build_bedrock_messages(&[]);
    assert!(messages.is_empty());
}

#[test]
fn test_bedrock_messages_with_history() {
    let history = vec![
        ChatMessage {
            chat_id: 123,
            message_id: 1,
            update_id: Some(100),
            user_id: Some(123),
            role: "user".to_string(),
            text: "Hola".to_string(),
            source: "telegram".to_string(),
            has_photo: false,
            reply_to_message_id: None,
            created_at: "2026-04-08T10:00:00Z".to_string(),
            created_at_epoch: 1775643600,
        },
        ChatMessage {
            chat_id: 123,
            message_id: 2,
            update_id: None,
            user_id: None,
            role: "assistant".to_string(),
            text: "Hola, soy un bot.".to_string(),
            source: "bedrock".to_string(),
            has_photo: false,
            reply_to_message_id: Some(1),
            created_at: "2026-04-08T10:00:02Z".to_string(),
            created_at_epoch: 1775643602,
        },
    ];

    let messages = ChatHistory::build_bedrock_messages(&history);

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "Hola");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "Hola, soy un bot.");
}

#[test]
fn test_bedrock_messages_photo_in_history() {
    let history = vec![
        ChatMessage {
            chat_id: 123,
            message_id: 1,
            update_id: Some(100),
            user_id: Some(123),
            role: "user".to_string(),
            text: "[Foto] Describe este diagrama".to_string(),
            source: "telegram".to_string(),
            has_photo: true,
            reply_to_message_id: None,
            created_at: "2026-04-08T10:00:00Z".to_string(),
            created_at_epoch: 1775643600,
        },
    ];

    let messages = ChatHistory::build_bedrock_messages(&history);

    assert_eq!(messages.len(), 1);
    let content = messages[0]["content"].as_str().unwrap();
    assert!(content.contains("foto"), "fotos en historial deben indicarse como texto");
    assert!(content.contains("Describe este diagrama"));
}

// ─── Tests de epoch_to_iso8601 ─────────────────────────────────────────────

#[test]
fn test_epoch_to_iso8601_known_date() {
    // 2026-04-08T10:20:23Z = epoch 1775643623
    let result = epoch_to_iso8601(1775643623);
    assert_eq!(result, "2026-04-08T10:20:23Z");
}

#[test]
fn test_epoch_to_iso8601_epoch_zero() {
    let result = epoch_to_iso8601(0);
    assert_eq!(result, "1970-01-01T00:00:00Z");
}

#[test]
fn test_epoch_to_iso8601_y2k() {
    // 2000-01-01T00:00:00Z = epoch 946684800
    let result = epoch_to_iso8601(946684800);
    assert_eq!(result, "2000-01-01T00:00:00Z");
}

// ─── Tests de build_model_body_with_context ────────────────────────────────

#[test]
fn test_build_model_body_with_context() {
    use stream_rust::build_model_body_with_context;

    let messages = vec![
        json!({"role": "user", "content": "Hola"}),
        json!({"role": "assistant", "content": "Hola!"}),
        json!({"role": "user", "content": "Como estas?"}),
    ];

    let body = build_model_body_with_context(2048, "You are helpful.", messages);

    assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
    assert_eq!(body["max_tokens"], 2048);
    assert_eq!(body["system"], "You are helpful.");
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
}
