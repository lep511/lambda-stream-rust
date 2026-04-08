/// Manejo de webhooks de Telegram para la Lambda de Bedrock streaming.
///
/// Este módulo detecta si un request entrante proviene de un webhook de Telegram,
/// extrae la información relevante del update, invoca Bedrock con el texto del
/// mensaje, y envía la respuesta de vuelta al chat de Telegram via la Bot API
/// con **streaming progresivo**: se envía un mensaje inicial y se edita
/// periódicamente a medida que llegan chunks de Bedrock.
///
/// Referencia: <https://core.telegram.org/bots/api#update>
use std::env;
use std::time::Instant;

use aws_sdk_bedrockruntime::{Client as BedrockClient, types::ResponseStream};
use aws_smithy_types::Blob;
use bytes::Bytes;
use http::{HeaderMap, StatusCode, header::HeaderValue};
use lambda_runtime::{Error, streaming::{Body, Response, channel}};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    default_max_tokens, default_model,
    extract_text_delta, streaming_response,
};
use crate::stream_markdown::md_to_telegram_markdownv2;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// Límite de caracteres por mensaje en la API de Telegram.
const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

/// Mínimo de caracteres acumulados antes de enviar el primer mensaje.
/// Evita que el mensaje inicial sea demasiado corto y parpadee en Telegram.
const INITIAL_MESSAGE_MIN_CHARS: usize = 15;

/// Intervalo mínimo entre ediciones del mensaje en Telegram (ms).
/// Evita rate-limiting de la Bot API (~30 req/s por chat).
const EDIT_THROTTLE_MS: u128 = 1000;

// ─── Tipos de Telegram ─────────────────────────────────────────────────────

/// Representa un Update de la Bot API de Telegram.
///
/// Un update es el objeto raíz que Telegram envía al webhook cada vez que
/// ocurre un evento relevante (mensaje nuevo, edición, callback, etc.).
///
/// Referencia: <https://core.telegram.org/bots/api#update>
#[derive(Debug, Deserialize)]
pub struct TelegramUpdate {
    /// Identificador único del update. Crece monótonamente.
    pub update_id: i64,
    /// Mensaje nuevo entrante (presente solo si el update es un mensaje de texto/media).
    pub message: Option<TelegramMessage>,
}

/// Representa un mensaje de Telegram.
///
/// Contiene el texto, el remitente, el chat al que pertenece y metadatos
/// como la fecha de envío y el ID del mensaje.
///
/// Referencia: <https://core.telegram.org/bots/api#message>
#[derive(Debug, Deserialize)]
pub struct TelegramMessage {
    /// Identificador único del mensaje dentro del chat.
    pub message_id: i64,
    /// Remitente del mensaje. Puede ser `None` en mensajes de canales.
    pub from: Option<TelegramUser>,
    /// Chat al que pertenece el mensaje.
    pub chat: TelegramChat,
    /// Fecha de envío del mensaje como Unix timestamp (segundos desde epoch).
    pub date: i64,
    /// Contenido de texto del mensaje. `None` si es una foto, sticker, etc.
    pub text: Option<String>,
    /// Array de tamaños de foto (presente si el mensaje es una foto).
    pub photo: Option<Vec<TelegramPhotoSize>>,
    /// Caption de una foto, video, documento, etc.
    pub caption: Option<String>,
}

/// Representa un usuario de Telegram.
///
/// Contiene información del perfil del remitente: nombre, idioma,
/// si es un bot, y su username opcional.
///
/// Referencia: <https://core.telegram.org/bots/api#user>
#[derive(Debug, Deserialize)]
pub struct TelegramUser {
    /// Identificador único del usuario.
    pub id: i64,
    /// `true` si el usuario es un bot.
    pub is_bot: bool,
    /// Nombre de pila del usuario.
    pub first_name: String,
    /// Apellido del usuario (opcional).
    pub last_name: Option<String>,
    /// Username de Telegram sin el `@` (opcional).
    pub username: Option<String>,
    /// Código de idioma IETF del usuario, e.g. "es", "en" (opcional).
    pub language_code: Option<String>,
}

/// Representa un chat de Telegram.
///
/// Puede ser un chat privado, grupo, supergrupo o canal.
/// Contiene el ID necesario para enviar respuestas y metadatos del chat.
///
/// Referencia: <https://core.telegram.org/bots/api#chat>
#[derive(Debug, Deserialize)]
pub struct TelegramChat {
    /// Identificador único del chat.
    pub id: i64,
    /// Nombre de pila en chats privados (opcional).
    pub first_name: Option<String>,
    /// Apellido en chats privados (opcional).
    pub last_name: Option<String>,
    /// Username del chat (opcional).
    pub username: Option<String>,
    /// Tipo de chat: `"private"`, `"group"`, `"supergroup"` o `"channel"`.
    #[serde(rename = "type")]
    pub chat_type: String,
}

/// Representa un tamaño de foto enviada por Telegram.
///
/// Telegram envía múltiples resoluciones de la misma foto.
/// La última en el array es la de mayor resolución.
///
/// Referencia: <https://core.telegram.org/bots/api#photosize>
#[derive(Debug, Deserialize)]
pub struct TelegramPhotoSize {
    /// Identificador del archivo, usado para descargar vía `getFile`.
    pub file_id: String,
    /// Identificador único del archivo.
    pub file_unique_id: String,
    /// Tamaño del archivo en bytes (opcional).
    pub file_size: Option<i64>,
    /// Ancho de la foto en píxeles.
    pub width: i32,
    /// Alto de la foto en píxeles.
    pub height: i32,
}

/// Respuesta de la Bot API de Telegram al enviar un mensaje.
///
/// Solo se extraen los campos necesarios para editar el mensaje
/// posteriormente (`result.message_id`).
///
/// Referencia: <https://core.telegram.org/bots/api#message>
#[derive(Debug, Deserialize)]
struct TelegramApiResponse {
    /// `true` si la operación fue exitosa.
    ok: bool,
    /// Objeto resultado con el mensaje enviado (presente si `ok` es `true`).
    result: Option<TelegramSentMessage>,
}

/// Mensaje enviado/editado devuelto por la Bot API.
#[derive(Debug, Deserialize)]
struct TelegramSentMessage {
    /// ID del mensaje enviado, necesario para `editMessageText`.
    message_id: i64,
}

/// Respuesta de `getFile` de la Bot API de Telegram.
#[derive(Debug, Deserialize)]
struct TelegramFileResponse {
    ok: bool,
    result: Option<TelegramFile>,
}

/// Objeto File retornado por `getFile`.
#[derive(Debug, Deserialize)]
struct TelegramFile {
    file_path: Option<String>,
}

/// Tipo de input del usuario: texto simple o foto con caption.
enum UserInput {
    Text(String),
    Photo { file_id: String, caption: String },
}

// ─── Detección ─────────────────────────────────────────────────────────────

/// Determina si un body JSON corresponde a un webhook update de Telegram.
///
/// Verifica que el JSON contenga `update_id` (número) y `message` (objeto)
/// en el nivel raíz. Estos campos son exclusivos del formato de Telegram
/// y no colisionan con `PromptRequest` (que usa `prompt` o `messages`).
///
/// # Argumentos
/// * `body` - El string JSON del body del request.
///
/// # Retorna
/// `true` si el body parece ser un update de Telegram.
pub fn is_telegram_update(body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    v.get("update_id").is_some_and(Value::is_number)
        && v.get("message").is_some_and(Value::is_object)
}

// ─── Handler principal de Telegram ─────────────────────────────────────────

/// Procesa un webhook update de Telegram usando **Lambda streaming**.
///
/// Retorna el 200 OK al webhook de Telegram **inmediatamente** via el canal
/// de streaming de Lambda, y luego procesa el mensaje en background con
/// `tokio::spawn`. Esto evita que Telegram haga timeout (~60s) y reintente
/// el webhook mientras Bedrock genera la respuesta.
///
/// El procesamiento en background:
/// 1. Envía `sendChatAction(typing)`.
/// 2. Invoca Bedrock streaming.
/// 3. Por cada chunk, acumula texto y edita el mensaje cada ~1 segundo.
/// 4. Edición final con el texto completo (sin cursor).
///
/// # Flujo de respuesta Lambda
/// ```text
/// Telegram webhook ──> Lambda
///   <- 200 OK "{}" (inmediato via channel)
///   [background] Bedrock stream -> editMessageText x N -> done
///   <- stream cerrado (tx dropped)
/// ```
///
/// # Argumentos
/// * `bedrock` - Cliente de Bedrock Runtime para invocar el modelo.
/// * `body_str` - Body JSON crudo del webhook de Telegram.
/// * `request_id` - ID del request Lambda para correlación de logs.
///
/// # Errores
/// Retorna `Error` solo en fallos irrecuperables de serialización.
/// Los errores de Bedrock o Telegram se manejan internamente y siempre
/// se retorna 200 al webhook para evitar reintentos.
pub async fn use_telegram(
    bedrock: &BedrockClient,
    body_str: &str,
    request_id: &str,
) -> Result<Response<Body>, Error> {
    let start = Instant::now();

    // 1. Parsear el update de Telegram
    let update: TelegramUpdate = match serde_json::from_str(body_str) {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(
                request_id,
                error = %e,
                latency_ms = start.elapsed().as_millis() as u64,
                "Telegram update inválido"
            );
            return Ok(ok_empty_response());
        }
    };

    // 2. Validar que exista un mensaje con texto
    let message = match &update.message {
        Some(m) => m,
        None => {
            tracing::info!(
                request_id,
                update_id = update.update_id,
                "update de Telegram sin message, ignorando"
            );
            return Ok(ok_empty_response());
        }
    };

    // 2. Determinar tipo de input: texto o foto
    let has_photo = message.photo.as_ref().is_some_and(|p| !p.is_empty());
    let user_input = if has_photo {
        let photos = message.photo.as_ref().unwrap();
        let largest = photos.last().unwrap();
        let caption = message.caption.clone()
            .unwrap_or_else(|| "Describe esta imagen".to_string());
        UserInput::Photo {
            file_id: largest.file_id.clone(),
            caption,
        }
    } else if let Some(ref t) = message.text {
        if t.is_empty() {
            tracing::info!(
                request_id,
                update_id = update.update_id,
                message_id = message.message_id,
                "mensaje de Telegram sin texto, ignorando"
            );
            return Ok(ok_empty_response());
        }
        UserInput::Text(t.clone())
    } else {
        tracing::info!(
            request_id,
            update_id = update.update_id,
            message_id = message.message_id,
            "mensaje de Telegram sin texto ni foto, ignorando"
        );
        return Ok(ok_empty_response());
    };

    let chat_id = message.chat.id;

    // 3. Leer el bot token de la variable de entorno
    let bot_token = match env::var("TELEGRAM_BOT_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            tracing::error!(request_id, "TELEGRAM_BOT_TOKEN no configurado");
            return Ok(ok_empty_response());
        }
    };

    let model_id = default_model();
    let max_tokens = default_max_tokens();

    // 5. Crear canal de streaming Lambda
    //    tx se mueve al spawn — Lambda mantiene la invocación abierta mientras tx exista.
    //    El send_data("{}" ) se hace DENTRO del spawn para que Lambda no cierre antes.
    let (tx, rx) = channel();

    // 6. Spawn: enviar 200 OK + procesar Bedrock + Telegram
    let rid = request_id.to_string();
    let bedrock = bedrock.clone();

    tokio::spawn(async move {
        let mut tx = tx; // tomar ownership explícito de tx
        let http_client = reqwest::Client::new();

        // 7.0 Enviar el body "{}" por el canal — Telegram recibe su 200 OK
        let _ = tx.send_data(Bytes::from_static(b"{}")).await;

        // 7a. Enviar typing indicator (sin placeholder visible)
        send_chat_action(&http_client, &bot_token, chat_id).await;

        // 7a.1 Construir payload de Bedrock (descarga imagen si es foto)
        let blob = match user_input {
            UserInput::Text(ref text) => {
                let body = json!({
                    "anthropic_version": "bedrock-2023-05-31",
                    "max_tokens": max_tokens,
                    "messages": [{"role": "user", "content": text}]
                });
                Blob::new(serde_json::to_vec(&body).unwrap())
            }
            UserInput::Photo { ref file_id, ref caption } => {
                let Some((image_data, media_type)) = download_telegram_photo(
                    &http_client, &bot_token, file_id,
                ).await else {
                    tracing::error!(
                        request_id = rid,
                        "no se pudo descargar la imagen de Telegram"
                    );
                    send_telegram_message(
                        &http_client, &bot_token, chat_id,
                        "No pude descargar la imagen\\. Intenta de nuevo\\.",
                    ).await;
                    return;
                };
                tracing::info!(
                    request_id = rid,
                    image_bytes = image_data.len(),
                    media_type = %media_type,
                    "imagen descargada de Telegram"
                );
                let b64 = BASE64.encode(&image_data);
                let body = json!({
                    "anthropic_version": "bedrock-2023-05-31",
                    "max_tokens": 4096,
                    "messages": [{
                        "role": "user",
                        "content": [
                            {
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": media_type,
                                    "data": b64,
                                }
                            },
                            {
                                "type": "text",
                                "text": caption,
                            }
                        ]
                    }]
                });
                Blob::new(serde_json::to_vec(&body).unwrap())
            }
        };

        // 7b. Invocar Bedrock con streaming
        tracing::info!(
            request_id = rid,
            model = %model_id,
            max_tokens,
            "invocando Bedrock streaming para Telegram"
        );

        let mut bedrock_stream = match bedrock
            .invoke_model_with_response_stream()
            .model_id(&model_id)
            .content_type("application/json")
            .body(blob)
            .send()
            .await
        {
            Ok(output) => {
                tracing::info!(
                    request_id = rid,
                    latency_ms = start.elapsed().as_millis() as u64,
                    "Bedrock stream abierto para Telegram"
                );
                output.body
            }
            Err(e) => {
                let raw = format!("{e:?}");
                tracing::error!(
                    request_id = rid,
                    error = %raw,
                    model = %model_id,
                    latency_ms = start.elapsed().as_millis() as u64,
                    "error al invocar Bedrock para Telegram"
                );
                send_telegram_message(
                    &http_client,
                    &bot_token,
                    chat_id,
                    "Error al generar la respuesta. Intenta de nuevo.",
                )
                .await;
                return;
            }
        };

        // 7c. Streaming progresivo con split en tiempo real por "\n---\n":
        //     - Cada segmento va en su propio mensaje de Telegram.
        //     - Al detectar "\n---\n", se finaliza el mensaje actual y se
        //       inicia uno nuevo para el siguiente segmento.
        let mut response_text = String::new();
        let mut last_sent_text = String::new();
        let mut segment_start: usize = 0;
        let mut chunk_count: u64 = 0;
        let mut edit_count: u64 = 0;
        let mut msg_id: Option<i64> = None;
        let stream_start = Instant::now();
        let mut last_edit = Instant::now();

        loop {
            match bedrock_stream.recv().await {
                Ok(Some(event)) => match event {
                    ResponseStream::Chunk(chunk) => {
                        if let Some(blob) = chunk.bytes
                            && let Some(text_delta) = extract_text_delta(blob.as_ref())
                        {
                            chunk_count += 1;
                            response_text.push_str(&text_delta);

                            // Detectar separadores "\n---\n" en tiempo real
                            while let Some(sep) = response_text[segment_start..].find("\n---\n") {
                                let abs_sep = segment_start + sep;
                                let segment = response_text[segment_start..abs_sep].trim();

                                if !segment.is_empty() {
                                    let display = truncate_for_telegram(
                                        &md_to_telegram_markdownv2(segment),
                                    );
                                    if let Some(mid) = msg_id {
                                        if display != last_sent_text {
                                            edit_telegram_message(
                                                &http_client, &bot_token, chat_id, mid, &display,
                                            ).await;
                                        }
                                    } else {
                                        send_telegram_message(
                                            &http_client, &bot_token, chat_id, &display,
                                        ).await;
                                    }
                                }

                                // Avanzar al siguiente segmento
                                segment_start = abs_sep + 5; // saltar "\n---\n"
                                msg_id = None;
                                last_sent_text = String::new();
                            }

                            // Streaming normal del segmento actual
                            let current_segment = response_text[segment_start..].trim();
                            if current_segment.is_empty() {
                                continue;
                            }

                            let display = truncate_for_telegram(
                                &md_to_telegram_markdownv2(current_segment),
                            );

                            if msg_id.is_none() {
                                // Acumular al menos 15 chars antes de enviar
                                // el primer mensaje (evita parpadeo)
                                if current_segment.len() < INITIAL_MESSAGE_MIN_CHARS {
                                    continue;
                                }
                                msg_id = send_telegram_message(
                                    &http_client, &bot_token, chat_id, &display,
                                ).await;
                                if msg_id.is_none() {
                                    tracing::error!(
                                        request_id = rid,
                                        chat_id,
                                        "no se pudo enviar mensaje inicial a Telegram"
                                    );
                                    return;
                                }
                                last_sent_text = display;
                                last_edit = Instant::now();
                            } else if last_edit.elapsed().as_millis() >= EDIT_THROTTLE_MS
                                && display != last_sent_text
                            {
                                edit_telegram_message(
                                    &http_client, &bot_token, chat_id,
                                    msg_id.unwrap(), &display,
                                ).await;
                                last_sent_text = display;
                                edit_count += 1;
                                last_edit = Instant::now();
                            }
                        }
                    }
                    _ => break,
                },
                Ok(None) => {
                    tracing::info!(
                        request_id = rid,
                        chunk_count,
                        edit_count,
                        total_bytes = response_text.len(),
                        duration_ms = stream_start.elapsed().as_millis() as u64,
                        "stream de Bedrock completado para Telegram"
                    );
                    break;
                }
                Err(e) => {
                    tracing::error!(
                        request_id = rid,
                        error = %e,
                        chunk_count,
                        edit_count,
                        total_bytes = response_text.len(),
                        duration_ms = stream_start.elapsed().as_millis() as u64,
                        "Bedrock stream error para Telegram"
                    );
                    if response_text.is_empty() {
                        response_text.push_str("Error al generar la respuesta.");
                    }
                    break;
                }
            }
        }

        // 7d. Edición final del último segmento
        let final_segment = response_text[segment_start..].trim();
        if !final_segment.is_empty() {
            let display = truncate_for_telegram(
                &md_to_telegram_markdownv2(final_segment),
            );
            if let Some(mid) = msg_id {
                if display != last_sent_text {
                    edit_telegram_message(
                        &http_client, &bot_token, chat_id, mid, &display,
                    ).await;
                }
            } else {
                send_telegram_message(
                    &http_client, &bot_token, chat_id, &display,
                ).await;
            }
        }

        tracing::info!(
            request_id = rid,
            chat_id,
            msg_id,
            response_len = response_text.len(),
            chunk_count,
            edit_count,
            total_duration_ms = start.elapsed().as_millis() as u64,
            "respuesta streaming completada en Telegram"
        );

        // tx se dropea aquí, cerrando el stream de Lambda
    });

    // 8. Retornar respuesta streaming — el 200 ya fue enviado por el canal
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    Ok(streaming_response(StatusCode::OK, headers, rx))
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Trunca texto MarkdownV2 si excede el límite de Telegram (4096 chars).
///
/// Los sufijos (indicador de truncado) ya están pre-escapados para
/// MarkdownV2. Al truncar, se evita cortar a mitad de una secuencia
/// de escape `\X`.
fn truncate_for_telegram(text: &str) -> String {
    // Pre-escaped para MarkdownV2: … → \.\.\., [truncado] → \[truncado\]
    let truncation = "\\.\\.\\. \\[truncado\\]";

    if text.len() > TELEGRAM_MAX_MESSAGE_LEN {
        let max_with_truncation = TELEGRAM_MAX_MESSAGE_LEN - truncation.len();
        // No cortar a mitad de una secuencia de escape \X
        let mut end = max_with_truncation;
        if end > 0 && text.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\\') {
            end -= 1;
        }
        let mut truncated = text[..end].to_string();
        truncated.push_str(truncation);
        truncated
    } else {
        text.to_string()
    }
}

/// Envía el indicador "typing..." al chat de Telegram.
///
/// Muestra al usuario que el bot está procesando su mensaje.
/// Los errores se loguean pero no se propagan.
///
/// Referencia: <https://core.telegram.org/bots/api#sendchataction>
async fn send_chat_action(client: &reqwest::Client, bot_token: &str, chat_id: i64) {
    let url = format!("https://api.telegram.org/bot{bot_token}/sendChatAction");
    let body = json!({
        "chat_id": chat_id,
        "action": "typing",
    });

    if let Err(e) = client.post(&url).json(&body).send().await {
        tracing::warn!(error = %e, "error al enviar typing indicator a Telegram");
    }
}

/// Envía un mensaje de texto a un chat de Telegram via la Bot API.
///
/// Realiza un POST a `https://api.telegram.org/bot{token}/sendMessage`
/// con el `chat_id` y `text` proporcionados. Retorna el `message_id`
/// del mensaje enviado si la operación fue exitosa, o `None` si falló.
///
/// El `message_id` es necesario para editar el mensaje posteriormente
/// con `editMessageText`.
///
/// # Argumentos
/// * `client` - Cliente HTTP reutilizable.
/// * `bot_token` - Token del bot de Telegram (sin el prefijo "bot").
/// * `chat_id` - ID del chat al que enviar el mensaje.
/// * `text` - Texto del mensaje a enviar.
///
/// # Retorna
/// `Some(message_id)` si el mensaje se envió correctamente, `None` si falló.
async fn send_telegram_message(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: i64,
    text: &str,
) -> Option<i64> {
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let body = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "MarkdownV2",
    });

    let resp = match client.post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "error al enviar mensaje a Telegram");
            return None;
        }
    };

    let status = resp.status();
    let resp_body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        tracing::warn!(
            status = %status,
            response = %resp_body,
            "Telegram sendMessage respondió con error"
        );
        // Fallback: si MarkdownV2 es inválido, reintentar sin parse_mode
        if status.as_u16() == 400 && resp_body.contains("can't parse entities") {
            tracing::info!("reintentando sendMessage sin parse_mode (fallback)");
            let fallback_body = json!({
                "chat_id": chat_id,
                "text": text,
            });
            if let Ok(fallback_resp) = client.post(&url).json(&fallback_body).send().await {
                let fb_body = fallback_resp.text().await.unwrap_or_default();
                if let Ok(api_resp) = serde_json::from_str::<TelegramApiResponse>(&fb_body) {
                    if api_resp.ok {
                        return api_resp.result.map(|r| r.message_id);
                    }
                }
            }
        }
        return None;
    }

    // Parsear la respuesta para obtener el message_id
    match serde_json::from_str::<TelegramApiResponse>(&resp_body) {
        Ok(api_resp) if api_resp.ok => api_resp.result.map(|r| r.message_id),
        Ok(_) => {
            tracing::warn!(response = %resp_body, "Telegram API retornó ok=false");
            None
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                response = %resp_body,
                "error al parsear respuesta de Telegram sendMessage"
            );
            None
        }
    }
}

/// Edita el texto de un mensaje existente en Telegram.
///
/// Usa `editMessageText` de la Bot API para actualizar el contenido
/// de un mensaje previamente enviado. Esto permite el efecto de
/// "streaming" donde el usuario ve el texto construyéndose progresivamente.
///
/// Los errores se loguean pero no se propagan, ya que una edición fallida
/// no debe interrumpir el flujo del stream.
///
/// Referencia: <https://core.telegram.org/bots/api#editmessagetext>
///
/// # Argumentos
/// * `client` - Cliente HTTP reutilizable.
/// * `bot_token` - Token del bot de Telegram.
/// * `chat_id` - ID del chat donde está el mensaje.
/// * `message_id` - ID del mensaje a editar.
/// * `text` - Nuevo texto del mensaje.
async fn edit_telegram_message(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: i64,
    message_id: i64,
    text: &str,
) {
    let url = format!("https://api.telegram.org/bot{bot_token}/editMessageText");
    let body = json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "text": text,
        "parse_mode": "MarkdownV2",
    });

    match client.post(&url).json(&body).send().await {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                let resp_body = resp.text().await.unwrap_or_default();
                tracing::warn!(
                    status = %status,
                    message_id,
                    response = %resp_body,
                    "Telegram editMessageText respondió con error"
                );
                // Fallback: si MarkdownV2 es inválido, reintentar sin parse_mode
                if status.as_u16() == 400 && resp_body.contains("can't parse entities") {
                    tracing::info!(message_id, "reintentando editMessageText sin parse_mode (fallback)");
                    let fallback_body = json!({
                        "chat_id": chat_id,
                        "message_id": message_id,
                        "text": text,
                    });
                    if let Err(e) = client.post(&url).json(&fallback_body).send().await {
                        tracing::error!(error = %e, message_id, "error en fallback editMessageText");
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                message_id,
                "error al editar mensaje en Telegram"
            );
        }
    }
}

/// Descarga una foto de Telegram dado su `file_id`.
///
/// 1. Llama a `getFile` para obtener el `file_path`.
/// 2. Descarga el archivo desde `https://api.telegram.org/file/bot{token}/{path}`.
///
/// Retorna `(bytes, media_type)` o `None` si algo falla.
async fn download_telegram_photo(
    client: &reqwest::Client,
    bot_token: &str,
    file_id: &str,
) -> Option<(Vec<u8>, String)> {
    // 1. Obtener file_path vía getFile
    let url = format!("https://api.telegram.org/bot{bot_token}/getFile");
    let body = json!({"file_id": file_id});
    let resp = match client.post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "error al llamar getFile de Telegram");
            return None;
        }
    };
    let file_resp: TelegramFileResponse = match resp.json().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "error al parsear respuesta de getFile");
            return None;
        }
    };
    if !file_resp.ok {
        tracing::warn!("getFile retornó ok=false");
        return None;
    }
    let file_path = file_resp.result?.file_path?;

    // 2. Determinar media type por extensión
    let media_type = if file_path.ends_with(".png") {
        "image/png"
    } else if file_path.ends_with(".gif") {
        "image/gif"
    } else if file_path.ends_with(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    };

    // 3. Descargar el archivo
    let download_url = format!("https://api.telegram.org/file/bot{bot_token}/{file_path}");
    let data = match client.get(&download_url).send().await {
        Ok(r) => match r.bytes().await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(error = %e, "error al descargar archivo de Telegram");
                return None;
            }
        },
        Err(e) => {
            tracing::error!(error = %e, "error al conectar para descargar archivo de Telegram");
            return None;
        }
    };

    Some((data.to_vec(), media_type.to_string()))
}

/// Construye una respuesta HTTP 200 OK con body vacío.
///
/// Telegram requiere que el webhook responda con 200 para confirmar
/// la recepción del update. Si se responde con otro código, Telegram
/// reintentará el envío del update.
fn ok_empty_response() -> Response<Body> {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    streaming_response(StatusCode::OK, headers, Body::from(Bytes::from_static(b"{}")))
}
