# Lambda Bedrock Streaming — Rust

## Arquitectura

```
Cliente (stream-client o curl)
  |  POST /prod/invoke  {"prompt":"..."}
  v
API Gateway REST
  |  responseTransferMode = STREAM
  |  credentials: ApiGatewayLambdaRole (assume role)
  v
Lambda (Rust, provided.al2023, arm64)
  |  lambda_runtime::run + StreamResponse
  |  lambda_runtime::streaming::channel() tx/rx
  v
Bedrock InvokeModelWithResponseStream
  |  chunked -> tx -> rx -> cliente
```

## Recursos desplegados (SAM)

| Recurso                   | Tipo                          | Descripcion                                          |
|---------------------------|-------------------------------|------------------------------------------------------|
| `BedrockStreamFunction`   | `AWS::Serverless::Function`   | Lambda Rust, provided.al2023, ARM64 (Graviton)       |
| `ApiGatewayLambdaRole`    | `AWS::IAM::Role`              | Rol que API Gateway asume para invocar Lambda         |
| `StreamApi`               | `AWS::Serverless::Api`        | API Gateway REST con OpenAPI y streaming habilitado   |
| `BedrockStreamLogGroup`   | `AWS::Logs::LogGroup`         | CloudWatch Logs con retencion de 14 dias              |

## Prerequisitos

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# AWS SAM CLI
pip3 install aws-sam-cli

# Habilitar modelo en Bedrock Console > Model access
# anthropic.claude-sonnet-4-6-v1:0
```

> **Nota:** El build usa `Makefile` + `cargo build` directamente (no `cargo-lambda`).
> Para compilar nativamente en ARM64 solo necesitas Rust y el target `aarch64-unknown-linux-gnu`.

## Build y Deploy

```bash
# Build (compila via Makefile, optimizado para Graviton4)
sam build

# Validar template
sam validate --lint

# Deploy (usa samconfig.toml con los parametros predefinidos)
sam deploy

# Deploy guiado (primera vez, genera samconfig.toml)
sam deploy --guided
```

### Optimizacion para Graviton4

El build esta optimizado para **AWS Graviton4 (Neoverse V2)** mediante tres archivos:

| Archivo | Funcion |
|---------|---------|
| `.cargo/config.toml` | `target-cpu=neoverse-v2` — habilita SVE2, BF16, I8MM, CSSC y crypto HW. Full RELRO y optimizacion del linker |
| `Cargo.toml` (profile.release) | `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true` — binario minimo con inlining agresivo |
| `Makefile` | Target `build-BedrockStreamFunction` invocado por `sam build` — compila para `aarch64-unknown-linux-gnu` y copia el artefacto |

SAM usa `BuildMethod: makefile` en el template, que ejecuta `make build-BedrockStreamFunction`. Las flags de `.cargo/config.toml` se aplican automaticamente.

El resultado es un binario de ~11 MB con instrucciones AES/SHA/PMULL nativas, acelerando el handshake TLS con Bedrock por hardware.

### Configuracion (samconfig.toml)

```toml
version = 0.1

[default.deploy.parameters]
stack_name = "rust-stream"
resolve_s3 = true
s3_prefix = "rust-stream"
capabilities = "CAPABILITY_NAMED_IAM"
parameter_overrides = "BedrockModelId=\"anthropic.claude-sonnet-4-6-v1:0\" LambdaMemorySize=\"256\" LambdaTimeout=\"120\""

[default.global.parameters]
region = "us-west-2"
```

> `CAPABILITY_NAMED_IAM` es necesario porque el template define roles IAM con nombres explicitos.

### Parametros del template

| Parametro          | Default                                | Descripcion                               |
|--------------------|----------------------------------------|-------------------------------------------|
| `BedrockModelId`   | `anthropic.claude-sonnet-4-6-v1:0`     | Model ID de Bedrock                       |
| `LambdaMemorySize` | `256`                                  | Memoria de la Lambda (MB)                 |
| `LambdaTimeout`    | `120`                                  | Timeout de la Lambda (segundos)           |

## Crear API Gateway REST (import)

Despues del deploy con SAM, ejecuta el script para crear la REST API importando la spec OpenAPI con los valores del stack:

```bash
./deploy-api.sh
```

El script:

1. Lee `region` y `stack_name` desde `samconfig.toml`
2. Obtiene `FunctionArn` y `ApiGatewayLambdaRoleArn` de los outputs del stack
3. Genera `temp_spec.yml` reemplazando `REPLACE_ME_1` (URI de integracion Lambda streaming) y `REPLACE_ME_2` (ARN del rol IAM) en `ApiSpec.yml`
4. Importa la API con `aws apigateway import-rest-api` (endpoint REGIONAL)
5. Imprime el `API_ID` resultante y elimina `temp_spec.yml`

## Eliminar stack

```bash
sam delete --stack-name rust-stream
```

## Politica IAM minima para el rol Lambda

```json
{
  "Effect": "Allow",
  "Action": [
    "bedrock:InvokeModel",
    "bedrock:InvokeModelWithResponseStream"
  ],
  "Resource": "arn:aws:bedrock:REGION::foundation-model/*"
}
```

## Pruebas

### Obtner API-ID

```bash
export API_ID=$(aws apigateway get-rest-apis \
  --region us-west-2 \
  --query "items[?name=='APIGW-ResponseStreamingRust'].id" \
  --output text)
```

### Obtener la URL del endpoint

```bash
export API_URL="https://${API_ID}.execute-api.us-west-2.amazonaws.com/prod/lambda"
echo $API_URL
```

### Prueba con curl (streaming)

```bash
# Streaming basico — respuesta en tiempo real
curl -i --no-buffer \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{"prompt":"Explica que es Rust en 3 parrafos"}' \
  "$API_URL"

# Con modelo y tokens personalizados
curl --no-buffer \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{"prompt":"Hola","model_id":"anthropic.claude-sonnet-4-6-v1:0","max_tokens":2048}' \
  "$API_URL"

# Verificar que el streaming funciona (los chunks llegan progresivamente)
# Si la respuesta aparece de golpe, revisar que responseTransferMode=STREAM
# este activo en la integracion de API Gateway
```

### Prueba con stream-client (Rust)

```bash
# Usar la URL del stack directamente
cargo run --bin stream -- "Explica que es Rust" --url "$API_URL"

# Con mas tokens
cargo run --bin stream -- "Cuando usar async en Rust?" \
  --url "$API_URL" \
  --max-tokens 2048

# Mostrar headers de respuesta (como curl -i)
cargo run --bin stream -- "Hola" --url "$API_URL" -i

# Usando API_ID y AWS_REGION (alternativa sin --url)
export API_ID=$(echo $API_URL | grep -oP '[a-z0-9]+(?=\.execute-api)')
export AWS_REGION=us-west-2
cargo run --bin stream -- "Hola"
```

### Prueba CORS (preflight)

```bash
curl -i -X OPTIONS \
  -H "Origin: http://localhost:3000" \
  -H "Access-Control-Request-Method: POST" \
  -H "Access-Control-Request-Headers: Content-Type" \
  "$API_URL"

# Debe retornar:
# Access-Control-Allow-Headers: Content-Type
# Access-Control-Allow-Methods: POST,OPTIONS
# Access-Control-Allow-Origin: *
```

### Pruebas unitarias

```bash
# Ejecutar todos los tests
cargo test

# Tests con output visible
cargo test -- --nocapture

# Test especifico
cargo test test_build_body
cargo test test_extract_delta
cargo test test_model_validation
cargo test test_prompt_request
cargo test test_responses
```

### Verificar logs en CloudWatch

Los logs son JSON estructurados (`LogFormat: JSON` en Globals) con campos para correlacion y diagnostico:

| Campo | Descripcion |
|-------|-------------|
| `request_id` | ID de la invocacion Lambda (presente en todos los logs) |
| `cold_start` | `true` en la primera invocacion del contenedor |
| `function_arn` | ARN de la funcion invocada |
| `model`, `max_tokens`, `message_count` | Parametros del request a Bedrock |
| `latency_ms` | Tiempo hasta que Bedrock abre el stream (o falla) |
| `chunk_count`, `total_bytes`, `duration_ms` | Metricas del streaming completado |

```bash
# Ver logs recientes de la Lambda
sam logs --stack-name rust-stream --tail

# Filtrar por errores
sam logs --stack-name rust-stream --filter "ERROR"

# Filtrar cold starts
sam logs --stack-name rust-stream --filter "cold_start"

# Filtrar por latencia de Bedrock
sam logs --stack-name rust-stream --filter "latency_ms"
```

## Modelo soportado

| Model ID                                      | Familia   |
|-----------------------------------------------|-----------|
| anthropic.claude-sonnet-4-6-v1:0              | Anthropic |

Activa el modelo en **Bedrock Console > Model access** antes de usarlo.

## Modelo de invocacion y streaming

Cada request HTTP POST ejecuta **una invocacion Lambda independiente**. El streaming no cambia esto — solo significa que la respuesta de esa unica invocacion se envia en chunks progresivos al cliente, en vez de esperar a completarse.

Lo que **si se reutiliza** entre invocaciones es el contenedor (warm start):

```
Request 1 (cold start)          Request 2 (warm)            Request 3 (warm)
+-----------------------+       +---------------------+     +---------------------+
| main() <- solo aqui   |       |                     |     |                     |
|   + init subscriber   |       |                     |     |                     |
|   + load aws_config   |       |                     |     |                     |
|   + create bedrock    |       |                     |     |                     |
|                       |       |                     |     |                     |
| handler() invocacion  |       | handler() nueva     |     | handler() nueva     |
|   + parsear body      |       |   + parsear body    |     |   + parsear body    |
|   + invocar Bedrock   |       |   + invocar Bedrock |     |   + invocar Bedrock |
|   + stream chunks     |       |   + stream chunks   |     |   + stream chunks   |
+-----------------------+       +---------------------+     +---------------------+
      ~100ms init                    ~0ms init                   ~0ms init
```

- **`main()`** corre una sola vez por contenedor: inicializa el `BedrockClient`, el tracing subscriber y el AWS config. Estos se reutilizan en invocaciones posteriores.
- **`handler()`** se ejecuta en cada invocacion pero reutiliza el cliente de Bedrock.
- El campo `cold_start` en los logs marca `true` solo la primera invocacion de cada contenedor.

## Costos del streaming

El streaming agrega un cobro por transferencia de datos **solo sobre los bytes que excedan los primeros 6 MB** de cada respuesta. Los costos normales de Lambda (invocaciones + duracion GB-s) aplican igual.

| Concepto | Valor |
|----------|-------|
| Respuesta buffered (sin streaming) | max 6 MB |
| Respuesta streaming | max 20 MB (soft limit, se puede aumentar) |
| Primeros 6 MB del stream | Sin costo adicional de bandwidth |
| Mas alla de 6 MB | Cobro por bytes transferidos, throughput max 2 MB/s (16 Mbps) |

En este proyecto las respuestas de Claude son texto que tipicamente no supera unos pocos KB, muy por debajo de los 6 MB. El streaming no agrega costo de transferencia — solo se paga lo estandar:

1. **Invocaciones** — por cada request
2. **Duracion** — GB-segundo (memoria asignada x tiempo de ejecucion)

El beneficio del streaming es mejorar el TTFB: el usuario ve texto llegando inmediatamente en vez de esperar a que Bedrock termine toda la respuesta, sin costo extra.

> Fuente: [Introducing AWS Lambda response streaming](https://aws.amazon.com/es/blogs/compute/introducing-aws-lambda-response-streaming/)

## Notas importantes

- En `lambda_runtime` 1.x, `run()` detecta streaming automaticamente cuando el handler retorna `Response<Body>`. Ya no se usa `run_with_streaming_response`.
- API Gateway REST requiere `responseTransferMode=STREAM` con `response-streaming-invocations` en el URI de integracion.
- La integracion usa `credentials` (rol IAM) para que API Gateway invoque la Lambda, en vez de `AWS::Lambda::Permission`.
- El canal `lambda_runtime::streaming::channel()` permite enviar chunks sin bloquear el handler.
- El `tokio::spawn` es necesario para que el handler retorne el `Response` con el receiver mientras el sender sigue enviando datos en background.
- Cold start con Rust + ARM64 (Graviton4) ~ 80-120ms.

## Concurrencia (provisioned concurrency)

Si configuras `AWS_LAMBDA_MAX_CONCURRENCY` (para provisioned concurrency con multi-request mode), debes usar `run_concurrent()` en lugar de `run()`. Esto requiere habilitar la feature `concurrency-tokio` en `lambda_runtime`:

```toml
lambda_runtime = { version = "1.1.2", features = ["concurrency-tokio"] }
```

```rust
// En main():
lambda_runtime::run_concurrent(service_fn(|ev| handler(&bedrock, ev))).await
```

Sin este cambio, la funcion retornara un error en runtime cuando `AWS_LAMBDA_MAX_CONCURRENCY` este activo.
