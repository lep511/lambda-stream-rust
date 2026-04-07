/// Lógica de negocio reutilizable para Lambda Bedrock Streaming.
///
/// Modelo soportado: Anthropic Claude Sonnet 4.6 vía Bedrock.
use std::env;

use bytes::Bytes;
use http::{HeaderMap, StatusCode, header::HeaderValue};
use lambda_runtime::{
    MetadataPrelude,
    streaming::{Body, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

// ─── Tipos de entrada ───────────────────────────────────────────────────────

/// Cuerpo que el cliente envía
#[derive(Debug, Deserialize)]
pub struct PromptRequest {
    /// Mensaje simple (se convierte en un solo mensaje de usuario)
    #[serde(default)]
    pub prompt: Option<String>,
    /// Historial completo de mensajes (tiene prioridad sobre prompt)
    #[serde(default)]
    pub messages: Option<Vec<Value>>,
    /// Modelo Bedrock a usar
    #[serde(default = "default_model")]
    pub model_id: String,
    /// Tokens máximos en la respuesta
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

pub fn default_model() -> String {
    env::var("BEDROCK_MODEL_ID").unwrap_or_else(|_| "anthropic.claude-sonnet-4-6-v1:0".to_string())
}

pub fn default_max_tokens() -> u32 {
    1024
}

// ─── Extracción de texto del stream de Bedrock ──────────────────────────────

/// Parsea un evento del stream de Bedrock y devuelve el texto delta, si lo hay.
///
/// Claude devuelve: `{"delta":{"type":"text_delta","text":"..."}}`
pub fn extract_text_delta(payload: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(payload).ok()?;

    v.pointer("/delta/text")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

// ─── Validación de model_id ─────────────────────────────────────────────────

pub fn is_known_model(model_id: &str) -> bool {
    model_id.contains("anthropic") || model_id.contains("claude")
}

// ─── Helpers de respuesta ───────────────────────────────────────────────────

/// Construye una respuesta de streaming con metadata (status + headers).
pub fn streaming_response(status: StatusCode, headers: HeaderMap, body: Body) -> Response<Body> {
    Response {
        metadata_prelude: MetadataPrelude {
            status_code: status,
            headers,
            cookies: Vec::new(),
        },
        stream: body,
    }
}

/// Respuesta de error con JSON body.
pub fn error_response(status: StatusCode, msg: &str) -> Response<Body> {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    let body = Body::from(Bytes::from(format!(r#"{{"error":"{msg}"}}"#)));
    streaming_response(status, headers, body)
}

// ─── Construcción del body para Claude (Messages API) ───────────────────────

pub fn build_model_body(req: &PromptRequest) -> Option<Value> {
    let messages = if let Some(ref msgs) = req.messages {
        msgs.clone()
    } else if let Some(ref prompt) = req.prompt {
        vec![json!({"role": "user", "content": prompt})]
    } else {
        return None;
    };

    Some(json!({
        "anthropic_version": "bedrock-2023-05-31",
        "max_tokens": req.max_tokens,
        "messages": messages
    }))
}
