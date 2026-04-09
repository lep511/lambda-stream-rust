/// Módulo de persistencia en DynamoDB para historial de chat.
///
/// Diseño de tabla (single-table):
///   PK = CHAT#<chat_id>
///   SK = TS#<epoch_millis>          → mensajes (user / assistant)
///   SK = METADATA#<chat_id>         → metadata del chat
///
/// TTL automático a 30 días sobre el atributo `ttl`.
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_dynamodb::types::AttributeValue;
use serde_json::{Value, json};

// ─── Constantes ────────────────────────────────────────────────────────────

/// TTL: 30 días en segundos.
const TTL_SECONDS: i64 = 30 * 24 * 3600;

// ─── Structs ───────────────────────────────────────────────────────────────

/// Servicio de acceso a la tabla de historial de chat.
#[derive(Clone)]
pub struct ChatHistory {
    client: DynamoDbClient,
    table_name: String,
}

/// Mensaje almacenado en DynamoDB (user o assistant).
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub chat_id: i64,
    pub message_id: i64,
    pub update_id: Option<i64>,
    pub user_id: Option<i64>,
    pub role: String,
    pub text: String,
    pub source: String,
    pub has_photo: bool,
    pub reply_to_message_id: Option<i64>,
    pub created_at: String,
    pub created_at_epoch: i64,
}

/// Metadata de un chat almacenada en DynamoDB.
#[derive(Debug, Clone)]
pub struct ChatMetadata {
    pub chat_id: i64,
    pub last_update_id: i64,
    pub last_message_at: String,
    pub first_name: String,
    pub language_code: Option<String>,
    pub message_count: i64,
    pub last_model: String,
    pub updated_at: String,
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Epoch actual en milisegundos.
pub fn now_epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Epoch actual en segundos.
pub fn now_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Formatea epoch segundos como ISO 8601 UTC (sin dependencia de chrono).
pub fn epoch_to_iso8601(epoch_secs: i64) -> String {
    // Cálculo manual simplificado: delegamos a una representación básica.
    // Para producción se podría usar chrono, pero mantenemos dependencias al mínimo.
    let secs = epoch_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Algoritmo de fecha civil desde días desde epoch
    // Referencia: https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn attr_s(val: &str) -> AttributeValue {
    AttributeValue::S(val.to_string())
}

fn attr_n(val: i64) -> AttributeValue {
    AttributeValue::N(val.to_string())
}

fn get_s(item: &HashMap<String, AttributeValue>, key: &str) -> Option<String> {
    item.get(key).and_then(|v| v.as_s().ok()).map(|s| s.to_string())
}

fn get_n(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key)
        .and_then(|v| v.as_n().ok())
        .and_then(|n| n.parse::<i64>().ok())
}

fn get_bool(item: &HashMap<String, AttributeValue>, key: &str) -> bool {
    item.get(key)
        .and_then(|v| v.as_bool().ok())
        .copied()
        .unwrap_or(false)
}

// ─── Implementación ────────────────────────────────────────────────────────

impl ChatHistory {
    pub fn new(client: DynamoDbClient, table_name: String) -> Self {
        Self { client, table_name }
    }

    /// Obtiene la metadata de un chat, si existe.
    pub async fn get_metadata(&self, chat_id: i64) -> Result<Option<ChatMetadata>, String> {
        let pk = format!("CHAT#{chat_id}");
        let sk = format!("METADATA#{chat_id}");

        let result = self.client.get_item()
            .table_name(&self.table_name)
            .key("PK", attr_s(&pk))
            .key("SK", attr_s(&sk))
            .send()
            .await
            .map_err(|e| format!("DynamoDB GetItem error: {e}"))?;

        let Some(item) = result.item else {
            return Ok(None);
        };

        Ok(Some(ChatMetadata {
            chat_id,
            last_update_id: get_n(&item, "last_update_id").unwrap_or(0),
            last_message_at: get_s(&item, "last_message_at").unwrap_or_default(),
            first_name: get_s(&item, "first_name").unwrap_or_default(),
            language_code: get_s(&item, "language_code"),
            message_count: get_n(&item, "message_count").unwrap_or(0),
            last_model: get_s(&item, "last_model").unwrap_or_default(),
            updated_at: get_s(&item, "updated_at").unwrap_or_default(),
        }))
    }

    /// Obtiene los últimos N mensajes de un chat en orden cronológico.
    pub async fn get_recent_messages(
        &self,
        chat_id: i64,
        limit: i32,
    ) -> Result<Vec<ChatMessage>, String> {
        let pk = format!("CHAT#{chat_id}");

        let result = self.client.query()
            .table_name(&self.table_name)
            .key_condition_expression("PK = :pk AND begins_with(SK, :prefix)")
            .expression_attribute_values(":pk", attr_s(&pk))
            .expression_attribute_values(":prefix", attr_s("TS#"))
            .scan_index_forward(false) // descendente → los más recientes primero
            .limit(limit)
            .send()
            .await
            .map_err(|e| format!("DynamoDB Query error: {e}"))?;

        let items = result.items();
        let mut messages: Vec<ChatMessage> = items
            .iter()
            .filter_map(|item| {
                Some(ChatMessage {
                    chat_id,
                    message_id: get_n(item, "message_id").unwrap_or(0),
                    update_id: get_n(item, "update_id"),
                    user_id: get_n(item, "user_id"),
                    role: get_s(item, "role")?,
                    text: get_s(item, "text").unwrap_or_default(),
                    source: get_s(item, "source").unwrap_or_default(),
                    has_photo: get_bool(item, "has_photo"),
                    reply_to_message_id: get_n(item, "reply_to_message_id"),
                    created_at: get_s(item, "created_at").unwrap_or_default(),
                    created_at_epoch: get_n(item, "created_at_epoch").unwrap_or(0),
                })
            })
            .collect();

        // Invertir para orden cronológico (ascendente)
        messages.reverse();
        Ok(messages)
    }

    /// Guarda un mensaje (user o assistant) en DynamoDB.
    pub async fn save_message(&self, msg: &ChatMessage) -> Result<(), String> {
        let pk = format!("CHAT#{}", msg.chat_id);
        let ts_millis = now_epoch_millis();
        let sk = format!("TS#{ts_millis}");
        let ttl = now_epoch_secs() + TTL_SECONDS;

        let mut item_builder = self.client.put_item()
            .table_name(&self.table_name)
            .item("PK", attr_s(&pk))
            .item("SK", attr_s(&sk))
            .item("chat_id", attr_n(msg.chat_id))
            .item("message_id", attr_n(msg.message_id))
            .item("role", attr_s(&msg.role))
            .item("text", attr_s(&msg.text))
            .item("source", attr_s(&msg.source))
            .item("created_at", attr_s(&msg.created_at))
            .item("created_at_epoch", attr_n(msg.created_at_epoch))
            .item("ttl", attr_n(ttl));

        if let Some(uid) = msg.user_id {
            item_builder = item_builder.item("user_id", attr_n(uid));
        }
        if let Some(uid) = msg.update_id {
            item_builder = item_builder.item("update_id", attr_n(uid));
        }
        if msg.has_photo {
            item_builder = item_builder.item("has_photo", AttributeValue::Bool(true));
        }
        if let Some(reply_id) = msg.reply_to_message_id {
            item_builder = item_builder.item("reply_to_message_id", attr_n(reply_id));
        }

        item_builder
            .send()
            .await
            .map_err(|e| format!("DynamoDB PutItem error: {e}"))?;

        Ok(())
    }

    /// Actualiza (o crea) la metadata de un chat con UpdateItem.
    pub async fn update_metadata(
        &self,
        chat_id: i64,
        update_id: i64,
        first_name: &str,
        language_code: Option<&str>,
        model_id: &str,
    ) -> Result<(), String> {
        let pk = format!("CHAT#{chat_id}");
        let sk = format!("METADATA#{chat_id}");
        let now_iso = epoch_to_iso8601(now_epoch_secs());

        let mut builder = self.client.update_item()
            .table_name(&self.table_name)
            .key("PK", attr_s(&pk))
            .key("SK", attr_s(&sk))
            .update_expression(
                "SET last_update_id = :uid, last_message_at = :now, \
                 first_name = :fname, last_model = :model, updated_at = :now \
                 ADD message_count :inc"
            )
            .expression_attribute_values(":uid", attr_n(update_id))
            .expression_attribute_values(":now", attr_s(&now_iso))
            .expression_attribute_values(":fname", attr_s(first_name))
            .expression_attribute_values(":model", attr_s(model_id))
            .expression_attribute_values(":inc", attr_n(1));

        if let Some(lang) = language_code {
            builder = builder
                .update_expression(
                    "SET last_update_id = :uid, last_message_at = :now, \
                     first_name = :fname, last_model = :model, updated_at = :now, \
                     language_code = :lang \
                     ADD message_count :inc"
                )
                .expression_attribute_values(":lang", attr_s(lang));
        }

        builder
            .send()
            .await
            .map_err(|e| format!("DynamoDB UpdateItem error: {e}"))?;

        Ok(())
    }

    /// Borra los mensajes de un chat (para el comando /clear).
    /// Solo elimina items con SK que empiece por "TS#" (mensajes),
    /// preservando la metadata (SK = "METADATA#...") y reseteando message_count a 0.
    pub async fn delete_chat_history(&self, chat_id: i64) -> Result<u32, String> {
        let pk = format!("CHAT#{chat_id}");
        let mut deleted: u32 = 0;
        let mut exclusive_start_key: Option<HashMap<String, AttributeValue>> = None;

        // 1. Borrar solo los mensajes (SK begins_with "TS#")
        loop {
            let mut query = self.client.query()
                .table_name(&self.table_name)
                .key_condition_expression("PK = :pk AND begins_with(SK, :prefix)")
                .expression_attribute_values(":pk", attr_s(&pk))
                .expression_attribute_values(":prefix", attr_s("TS#"))
                .projection_expression("PK, SK");

            if let Some(ref key) = exclusive_start_key {
                query = query.set_exclusive_start_key(Some(key.clone()));
            }

            let result = query.send().await
                .map_err(|e| format!("DynamoDB Query error: {e}"))?;

            let items = result.items();
            if items.is_empty() {
                break;
            }

            // BatchWriteItem acepta máximo 25 items por request
            for chunk in items.chunks(25) {
                use aws_sdk_dynamodb::types::{WriteRequest, DeleteRequest};

                let requests: Vec<WriteRequest> = chunk
                    .iter()
                    .filter_map(|item| {
                        let pk_val = item.get("PK")?.clone();
                        let sk_val = item.get("SK")?.clone();
                        let mut key = HashMap::new();
                        key.insert("PK".to_string(), pk_val);
                        key.insert("SK".to_string(), sk_val);
                        Some(
                            WriteRequest::builder()
                                .delete_request(
                                    DeleteRequest::builder()
                                        .set_key(Some(key))
                                        .build()
                                        .ok()?,
                                )
                                .build(),
                        )
                    })
                    .collect();

                deleted += requests.len() as u32;

                self.client.batch_write_item()
                    .request_items(&self.table_name, requests)
                    .send()
                    .await
                    .map_err(|e| format!("DynamoDB BatchWriteItem error: {e}"))?;
            }

            exclusive_start_key = result.last_evaluated_key().map(|k| k.to_owned());
            if exclusive_start_key.is_none() {
                break;
            }
        }

        // 2. Resetear message_count a 0 en la metadata (sin borrarla)
        let sk = format!("METADATA#{chat_id}");
        self.client.update_item()
            .table_name(&self.table_name)
            .key("PK", attr_s(&pk))
            .key("SK", attr_s(&sk))
            .update_expression("SET message_count = :zero")
            .expression_attribute_values(":zero", attr_n(0))
            .send()
            .await
            .map_err(|e| format!("DynamoDB UpdateItem error (reset message_count): {e}"))?;

        Ok(deleted)
    }

    // ─── Funciones puras (sin I/O) ────────────────────────────────────────

    /// Convierte el historial de mensajes a formato Messages API de Bedrock.
    pub fn build_bedrock_messages(history: &[ChatMessage]) -> Vec<Value> {
        history
            .iter()
            .map(|msg| {
                let content = if msg.has_photo && msg.role == "user" {
                    format!("[El usuario envió una foto con caption: {}]", msg.text)
                } else {
                    msg.text.clone()
                };
                json!({
                    "role": msg.role,
                    "content": content
                })
            })
            .collect()
    }

    /// Construye el system prompt con contexto del usuario.
    pub fn build_system_prompt(
        metadata: &Option<ChatMetadata>,
        user_name: &str,
        language: &Option<String>,
    ) -> String {
        let now = epoch_to_iso8601(now_epoch_secs());
        let mut prompt = format!(
            "You are a helpful AI assistant in a Telegram chat.\n\
             Current date and time (UTC): {now}."
        );

        if !user_name.is_empty() {
            prompt.push_str(&format!("\nThe user's name is {user_name}."));
        }

        if let Some(lang) = language {
            prompt.push_str(&format!(
                "\nThe user's language preference is '{lang}'. \
                 Respond in that language unless asked otherwise."
            ));
        }

        if let Some(meta) = metadata {
            if meta.message_count > 0 {
                prompt.push_str(&format!(
                    "\nYou have exchanged {} messages with this user previously.",
                    meta.message_count
                ));
            }
        }

        prompt
    }
}
