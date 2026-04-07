/// AWS Lambda + Bedrock streaming response in Rust
///
/// Arquitectura:
///   API Gateway (REST, responseTransferMode=STREAM)
///     └─> Lambda (run + StreamResponse)
///           └─> Bedrock InvokeModelWithResponseStream
///
/// Build:   cargo lambda build --release
/// Deploy:  cargo lambda deploy
/// IAM:     bedrock:InvokeModelWithResponseStream en el rol de ejecución
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use aws_sdk_bedrockruntime::{Client as BedrockClient, types::ResponseStream};
use aws_smithy_types::Blob;
use bytes::Bytes;
use http::{HeaderMap, StatusCode, header::HeaderValue};
use lambda_runtime::{
    Error, LambdaEvent, service_fn,
    streaming::{Body, Response, channel},
    tracing,
};
use serde::Deserialize;
use serde_json::Value;
use stream_rust::{
    PromptRequest, build_model_body, error_response, extract_text_delta, is_known_model,
    streaming_response,
};

static COLD_START: AtomicBool = AtomicBool::new(true);

// ─── Tipos de entrada ───────────────────────────────────────────────────────

/// Payload que llega vía API Gateway proxy (body JSON)
#[derive(Debug, Deserialize)]
struct ApiGatewayEvent {
    body: Option<String>,
}

// ─── Handler principal ──────────────────────────────────────────────────────

async fn handler(
    bedrock: &BedrockClient,
    event: LambdaEvent<Value>,
) -> Result<Response<Body>, Error> {
    let request_id = &event.context.request_id;
    let start = Instant::now();
    let cold = COLD_START.swap(false, Ordering::Relaxed);

    tracing::info!(
        request_id,
        cold_start = cold,
        function_arn = %event.context.invoked_function_arn,
        event = %serde_json::to_string(&event.payload).unwrap_or_default(),
        "request iniciado"
    );

    // 1. Parsear el evento de API Gateway
    let raw: ApiGatewayEvent = match serde_json::from_value(event.payload.clone()) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                request_id,
                error = %e,
                latency_ms = start.elapsed().as_millis() as u64,
                "evento de API Gateway inválido"
            );
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                &format!("evento inválido: {e}"),
            ));
        }
    };

    let body_str = match raw.body {
        Some(b) => b,
        None => {
            tracing::warn!(
                request_id,
                latency_ms = start.elapsed().as_millis() as u64,
                "body vacío en el evento de API Gateway"
            );
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                "body vacío en el evento de API Gateway",
            ));
        }
    };

    // Detectar si es un webhook de Telegram y derivar al handler específico
    if stream_rust::telegram::is_telegram_update(&body_str) {
        tracing::info!(request_id, "webhook de Telegram detectado");
        return stream_rust::telegram::use_telegram(bedrock, &body_str, request_id).await;
    }

    let req: PromptRequest = match serde_json::from_str(&body_str) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                request_id,
                error = %e,
                body_preview = %&body_str[..body_str.len().min(200)],
                latency_ms = start.elapsed().as_millis() as u64,
                "JSON de body inválido"
            );
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                &format!("JSON de body inválido: {e}"),
            ));
        }
    };

    if !is_known_model(&req.model_id) {
        tracing::warn!(
            request_id,
            model = %req.model_id,
            latency_ms = start.elapsed().as_millis() as u64,
            "modelo no reconocido"
        );
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            &format!(
                "modelo '{}' no soportado. Solo se admite anthropic/claude (Sonnet 4.6)",
                req.model_id
            ),
        ));
    }

    let msg_count = req.messages.as_ref().map_or(
        if req.prompt.is_some() { 1 } else { 0 },
        |m| m.len(),
    );
    tracing::info!(
        request_id,
        model = %req.model_id,
        max_tokens = req.max_tokens,
        message_count = msg_count,
        "invocando Bedrock streaming"
    );

    // 2. Construir el payload según el modelo
    let bedrock_body = match build_model_body(&req) {
        Some(body) => body,
        None => {
            tracing::warn!(
                request_id,
                model = %req.model_id,
                has_prompt = req.prompt.is_some(),
                has_messages = req.messages.is_some(),
                latency_ms = start.elapsed().as_millis() as u64,
                "body sin 'prompt' ni 'messages'"
            );
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                "se requiere 'prompt' o 'messages'",
            ));
        }
    };
    let blob = Blob::new(serde_json::to_vec(&bedrock_body)?);

    // 3. Invocar Bedrock con streaming
    let mut bedrock_stream = match bedrock
        .invoke_model_with_response_stream()
        .model_id(&req.model_id)
        .content_type("application/json")
        .body(blob)
        .send()
        .await
    {
        Ok(output) => {
            tracing::info!(
                request_id,
                latency_ms = start.elapsed().as_millis() as u64,
                "Bedrock stream abierto"
            );
            output.body
        }
        Err(e) => {
            let raw = format!("{e:?}");
            tracing::error!(
                request_id,
                error = %raw,
                model = %req.model_id,
                max_tokens = req.max_tokens,
                message_count = msg_count,
                latency_ms = start.elapsed().as_millis() as u64,
                "error al invocar Bedrock"
            );
            return Ok(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("error al invocar Bedrock: {raw}"),
            ));
        }
    };

    // 4. Crear canal para el streaming de respuesta Lambda
    let (mut tx, rx) = channel();

    // 5. Spawn: lee Bedrock y escribe en el canal del response
    let rid = request_id.clone();
    let stream_start = Instant::now();
    tokio::spawn(async move {
        let mut chunk_count: u64 = 0;
        let mut total_bytes: u64 = 0;
        loop {
            match bedrock_stream.recv().await {
                Ok(Some(event)) => match event {
                    ResponseStream::Chunk(chunk) => {
                        if let Some(blob) = chunk.bytes
                            && let Some(text) = extract_text_delta(blob.as_ref())
                        {
                            chunk_count += 1;
                            total_bytes += text.len() as u64;
                            if tx.send_data(Bytes::from(text)).await.is_err() {
                                tracing::warn!(
                                    request_id = rid,
                                    chunk_count,
                                    "cliente desconectado durante streaming"
                                );
                                break;
                            }
                        }
                    }
                    _ => break,
                },
                Ok(None) => {
                    tracing::info!(
                        request_id = rid,
                        chunk_count,
                        total_bytes,
                        duration_ms = stream_start.elapsed().as_millis() as u64,
                        "stream completado"
                    );
                    break;
                }
                Err(e) => {
                    tracing::error!(
                        request_id = rid,
                        error = %e,
                        chunk_count,
                        total_bytes,
                        duration_ms = stream_start.elapsed().as_millis() as u64,
                        "Bedrock stream error durante lectura de chunks"
                    );
                    let _ = tx.send_data(Bytes::from(format!("\n[ERROR: {e}]"))).await;
                    break;
                }
            }
        }
    });

    // 6. Devolver la respuesta con el receiver del canal
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    headers.insert("cache-control", HeaderValue::from_static("no-cache"));
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("POST, OPTIONS"),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static("Content-Type"),
    );

    Ok(streaming_response(StatusCode::OK, headers, rx))
}

// ─── Entry point ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    let aws_config = aws_config::from_env().load().await;
    let bedrock = BedrockClient::new(&aws_config);

    // run() detecta streaming automáticamente por el tipo StreamResponse<Body>
    lambda_runtime::run(service_fn(|ev: LambdaEvent<Value>| handler(&bedrock, ev))).await
}
