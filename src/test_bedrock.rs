use aws_config;
use aws_sdk_bedrockruntime::{Client, primitives::Blob};
use serde_json::json;

#[tokio::main]
async fn main() {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = Client::new(&config);

    let model_id = std::env::var("BEDROCK_MODEL_ID")
        .unwrap_or_else(|_| "us.anthropic.claude-sonnet-4-20250514-v1:0".to_string());

    let body = json!({
        "anthropic_version": "bedrock-2023-05-31",
        "max_tokens": 256,
        "messages": [{"role": "user", "content": "Hola, responde en una linea"}]
    });

    println!("Model: {model_id}");
    println!("Invocando Bedrock...");

    match client
        .invoke_model()
        .model_id(&model_id)
        .content_type("application/json")
        .body(Blob::new(serde_json::to_vec(&body).unwrap()))
        .send()
        .await
    {
        Ok(resp) => {
            let bytes = resp.body().as_ref();
            let parsed: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            println!("Respuesta: {}", serde_json::to_string_pretty(&parsed).unwrap());
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
        }
    }
}
