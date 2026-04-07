/// stream-client
///
/// Uso:
///   export API_ID=xxxx
///   export AWS_REGION=us-east-1
///   cargo run -- "¿Qué es Rust?"
///   cargo run -- "¿Qué es Rust?" --model anthropic.claude-sonnet-4-6-v1:0
///
/// Cargo.toml (bin separado o workspace):
/// [dependencies]
/// reqwest  = { version = "0.12", features = ["stream"] }
/// tokio    = { version = "1",    features = ["full"] }
/// serde_json = "1"
/// futures-util = "0.3"
/// clap     = { version = "4", features = ["derive"] }
use std::env;

use clap::Parser;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;

#[derive(Parser, Debug)]
#[command(
    name = "stream-client",
    about = "Prueba de streaming Lambda vía API Gateway"
)]
struct Args {
    /// Prompt a enviar
    prompt: String,

    /// Modelo Bedrock (opcional, el Lambda usa su default si se omite)
    #[arg(long)]
    model: Option<String>,

    /// Tokens máximos
    #[arg(long, default_value_t = 1024)]
    max_tokens: u32,

    /// URL completa del endpoint (sobreescribe API_ID + AWS_REGION)
    #[arg(long)]
    url: Option<String>,

    /// Timeout en segundos para la conexión
    #[arg(long, default_value_t = 120)]
    timeout: u64,

    /// Mostrar cabeceras de respuesta (como curl -i)
    #[arg(short = 'i', long)]
    headers: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // ── Resolver URL ──────────────────────────────────────────────────────
    let endpoint = match args.url {
        Some(u) => u,
        None => {
            let api_id = match env::var("API_ID") {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("Error: necesitas definir API_ID o usar --url");
                    std::process::exit(1);
                }
            };
            let region = env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
            format!("https://{api_id}.execute-api.{region}.amazonaws.com/prod/lambda")
        }
    };

    // ── Construir body ────────────────────────────────────────────────────
    let mut body = json!({
        "prompt":     args.prompt,
        "max_tokens": args.max_tokens,
    });

    if let Some(model) = args.model {
        body["model_id"] = json!(model);
    }

    eprintln!("→ POST {endpoint}");
    eprintln!("→ body: {body}");
    eprintln!("─────────────────────────────────────────");

    // ── Petición HTTP con streaming ───────────────────────────────────────
    let client = Client::builder()
        // Sin timeout de lectura: la respuesta puede tardar
        .timeout(std::time::Duration::from_secs(args.timeout))
        .build()?;

    let response = client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        // Equivale a --no-buffer de curl: no almacenar en buffer del cliente
        .header("Accept", "text/plain")
        .json(&body)
        .send()
        .await?;

    // ── Mostrar cabeceras (como curl -i) ──────────────────────────────────
    if args.headers {
        eprintln!("HTTP/{:?} {}", response.version(), response.status());
        for (k, v) in response.headers() {
            eprintln!("{}: {}", k, v.to_str().unwrap_or("?"));
        }
        eprintln!("─────────────────────────────────────────");
    }

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await?;
        eprintln!("Error {status}: {body}");
        std::process::exit(1);
    }

    // ── Leer stream de chunks e imprimir en tiempo real ───────────────────
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        // Imprimir cada chunk sin newline para respetar el flujo del modelo
        print!("{}", String::from_utf8_lossy(&chunk));
        // flush inmediato = --no-buffer equivalente
        use std::io::Write;
        std::io::stdout().flush()?;
    }

    println!(); // newline final
    Ok(())
}
