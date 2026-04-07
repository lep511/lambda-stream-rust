use stream_rust::telegram::{
    TelegramChat, TelegramUpdate, TelegramUser, is_telegram_update,
};

// ─── is_telegram_update: detección positiva ────────────────────────────────

#[test]
fn detects_valid_telegram_update() {
    let body = r#"{
        "update_id": 40965989,
        "message": {
            "message_id": 4600,
            "from": {"id": 123, "is_bot": false, "first_name": "Test"},
            "chat": {"id": 123, "first_name": "Test", "type": "private"},
            "date": 1775569249,
            "text": "Hola"
        }
    }"#;
    assert!(is_telegram_update(body));
}

#[test]
fn detects_update_with_minimal_message() {
    let body = r#"{
        "update_id": 1,
        "message": {
            "message_id": 1,
            "chat": {"id": 1, "type": "private"},
            "date": 0
        }
    }"#;
    assert!(is_telegram_update(body));
}

// ─── is_telegram_update: detección negativa ────────────────────────────────

#[test]
fn rejects_prompt_request_body() {
    let body = r#"{"prompt": "Hola", "model_id": "anthropic.claude-sonnet-4-6-v1:0"}"#;
    assert!(!is_telegram_update(body));
}

#[test]
fn rejects_messages_request_body() {
    let body = r#"{"messages": [{"role": "user", "content": "Hola"}]}"#;
    assert!(!is_telegram_update(body));
}

#[test]
fn rejects_empty_json() {
    assert!(!is_telegram_update("{}"));
}

#[test]
fn rejects_invalid_json() {
    assert!(!is_telegram_update("not json at all"));
}

#[test]
fn rejects_update_id_as_string() {
    let body = r#"{"update_id": "not_a_number", "message": {}}"#;
    assert!(!is_telegram_update(body));
}

#[test]
fn rejects_message_as_string() {
    let body = r#"{"update_id": 1, "message": "not an object"}"#;
    assert!(!is_telegram_update(body));
}

#[test]
fn rejects_missing_message() {
    let body = r#"{"update_id": 1}"#;
    assert!(!is_telegram_update(body));
}

#[test]
fn rejects_missing_update_id() {
    let body = r#"{"message": {"message_id": 1, "chat": {"id": 1, "type": "private"}, "date": 0}}"#;
    assert!(!is_telegram_update(body));
}

// ─── Deserialización de TelegramUpdate ─────────────────────────────────────

#[test]
fn parses_full_telegram_update() {
    let body = r#"{
        "update_id": 40965989,
        "message": {
            "message_id": 4600,
            "from": {
                "id": 795876358,
                "is_bot": false,
                "first_name": "Esteban",
                "last_name": "Perez",
                "username": "estebanbot",
                "language_code": "es"
            },
            "chat": {
                "id": 795876358,
                "first_name": "Esteban",
                "last_name": "Perez",
                "username": "estebanbot",
                "type": "private"
            },
            "date": 1775569249,
            "text": "Hi"
        }
    }"#;

    let update: TelegramUpdate = serde_json::from_str(body).unwrap();

    assert_eq!(update.update_id, 40965989);

    let msg = update.message.unwrap();
    assert_eq!(msg.message_id, 4600);
    assert_eq!(msg.date, 1775569249);
    assert_eq!(msg.text.as_deref(), Some("Hi"));

    let from = msg.from.unwrap();
    assert_eq!(from.id, 795876358);
    assert!(!from.is_bot);
    assert_eq!(from.first_name, "Esteban");
    assert_eq!(from.last_name.as_deref(), Some("Perez"));
    assert_eq!(from.username.as_deref(), Some("estebanbot"));
    assert_eq!(from.language_code.as_deref(), Some("es"));

    assert_eq!(msg.chat.id, 795876358);
    assert_eq!(msg.chat.first_name.as_deref(), Some("Esteban"));
    assert_eq!(msg.chat.last_name.as_deref(), Some("Perez"));
    assert_eq!(msg.chat.username.as_deref(), Some("estebanbot"));
    assert_eq!(msg.chat.chat_type, "private");
}

#[test]
fn parses_update_without_optional_fields() {
    let body = r#"{
        "update_id": 1,
        "message": {
            "message_id": 100,
            "chat": {"id": 42, "type": "group"},
            "date": 1000000
        }
    }"#;

    let update: TelegramUpdate = serde_json::from_str(body).unwrap();

    assert_eq!(update.update_id, 1);
    let msg = update.message.unwrap();
    assert_eq!(msg.message_id, 100);
    assert!(msg.from.is_none());
    assert!(msg.text.is_none());
    assert_eq!(msg.chat.id, 42);
    assert_eq!(msg.chat.chat_type, "group");
    assert!(msg.chat.first_name.is_none());
    assert!(msg.chat.username.is_none());
}

#[test]
fn parses_update_without_message() {
    let body = r#"{"update_id": 5}"#;
    let update: TelegramUpdate = serde_json::from_str(body).unwrap();

    assert_eq!(update.update_id, 5);
    assert!(update.message.is_none());
}

// ─── Deserialización de TelegramUser ───────────────────────────────────────

#[test]
fn parses_bot_user() {
    let json = r#"{"id": 999, "is_bot": true, "first_name": "MyBot"}"#;
    let user: TelegramUser = serde_json::from_str(json).unwrap();

    assert_eq!(user.id, 999);
    assert!(user.is_bot);
    assert_eq!(user.first_name, "MyBot");
    assert!(user.last_name.is_none());
    assert!(user.username.is_none());
    assert!(user.language_code.is_none());
}

// ─── Deserialización de TelegramChat ───────────────────────────────────────

#[test]
fn parses_supergroup_chat() {
    let json = r#"{
        "id": -1001234567890,
        "first_name": null,
        "username": "mygroup",
        "type": "supergroup"
    }"#;
    let chat: TelegramChat = serde_json::from_str(json).unwrap();

    assert_eq!(chat.id, -1001234567890);
    assert!(chat.first_name.is_none());
    assert_eq!(chat.username.as_deref(), Some("mygroup"));
    assert_eq!(chat.chat_type, "supergroup");
}

#[test]
fn parses_channel_chat() {
    let json = r#"{"id": -100999, "type": "channel"}"#;
    let chat: TelegramChat = serde_json::from_str(json).unwrap();

    assert_eq!(chat.id, -100999);
    assert_eq!(chat.chat_type, "channel");
}

// ─── Texto con caracteres especiales ───────────────────────────────────────

#[test]
fn parses_message_with_unicode_text() {
    let body = r#"{
        "update_id": 1,
        "message": {
            "message_id": 1,
            "chat": {"id": 1, "type": "private"},
            "date": 0,
            "text": "Hola 🎉 ñ ü é — «»"
        }
    }"#;

    let update: TelegramUpdate = serde_json::from_str(body).unwrap();
    assert_eq!(
        update.message.unwrap().text.as_deref(),
        Some("Hola 🎉 ñ ü é — «»")
    );
}

#[test]
fn parses_message_with_multiline_text() {
    let body = r#"{
        "update_id": 1,
        "message": {
            "message_id": 1,
            "chat": {"id": 1, "type": "private"},
            "date": 0,
            "text": "línea 1\nlínea 2\nlínea 3"
        }
    }"#;

    let update: TelegramUpdate = serde_json::from_str(body).unwrap();
    assert_eq!(
        update.message.unwrap().text.as_deref(),
        Some("línea 1\nlínea 2\nlínea 3")
    );
}
