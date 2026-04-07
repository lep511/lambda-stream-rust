# Lambda Bedrock Streaming — Rust

## Arquitectura

```
                          ┌─────────────────────┐
                          │   Telegram Bot API   │
                          │  (webhook -> Lambda) │
                          └────────┬────────────┘
                                   │ POST /webhook
                                   v
Cliente (curl / stream-client)    Lambda (Rust, provided.al2023, arm64)
  │  POST /prod/invoke            ├─ Detecta formato del body:
  │  {"prompt":"..."}             │   ├─ PromptRequest  -> streaming directo al cliente
  v                               │   └─ TelegramUpdate -> streaming a Telegram via Bot API
API Gateway REST                  │
  │  responseTransferMode=STREAM  │  lambda_runtime::streaming::channel() tx/rx
  v                               v
                          Bedrock InvokeModelWithResponseStream
                            │  chunked -> tx -> rx -> cliente / editMessageText
```

### Flujo directo (API Gateway)

```
Cliente -> API Gateway -> Lambda -> Bedrock stream -> channel tx/rx -> Cliente (chunks)
```

### Flujo Telegram (webhook)

```
Telegram webhook -> Lambda
  <- 200 OK "{}" (inmediato via channel, evita timeout de Telegram)
  [tokio::spawn] sendChatAction(typing)
               -> sendMessage(placeholder "▍") -> obtiene message_id
               -> Bedrock stream -> editMessageText cada ~1s (streaming progresivo)
               -> editMessageText final (texto completo)
  <- stream cerrado (tx dropped)
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
# Build (compila via Makefile, optimizado para Graviton2)
sam build

# Validar template
sam validate --lint

# Deploy (usa samconfig.toml con los parametros predefinidos)
sam deploy

# Deploy guiado (primera vez, genera samconfig.toml)
sam deploy --guided

# Deploy con token de Telegram
sam deploy --parameter-overrides TelegramBotToken=<tu-token>
```

### Optimizacion para Graviton

El build esta optimizado para **AWS Graviton2 (Neoverse N1)**, que es el procesador que usa Lambda arm64:

| Archivo | Funcion |
|---------|---------|
| `.cargo/config.toml` | `target-cpu=neoverse-n1` — compatible con Lambda arm64 (Graviton2). Full RELRO y optimizacion del linker |
| `Cargo.toml` (profile.release) | `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true` — binario minimo con inlining agresivo |
| `Makefile` | Target `build-BedrockStreamFunction` invocado por `sam build` — compila para `aarch64-unknown-linux-gnu` y copia el artefacto |

> **Importante:** Lambda arm64 usa Graviton2 (Neoverse N1), no Graviton4. Usar `target-cpu=neoverse-v2` genera instrucciones SVE2/BF16 que causan `illegal instruction` en Lambda.

SAM usa `BuildMethod: makefile` en el template, que ejecuta `make build-BedrockStreamFunction`. Las flags de `.cargo/config.toml` se aplican automaticamente.

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
| `TelegramBotToken` | `""` (NoEcho)                          | Token del bot de Telegram para Bot API    |
| `LambdaMemorySize` | `256`                                  | Memoria de la Lambda (MB)                 |
| `LambdaTimeout`    | `120`                                  | Timeout de la Lambda (segundos)           |

## Integracion con Telegram

La Lambda detecta automaticamente si el body es un webhook de Telegram (presencia de `update_id` + `message`) y lo procesa con streaming progresivo.

### Configurar el webhook de Telegram

#### 1. Crear Function URL para la Lambda

```bash
# Crear Function URL con CORS
aws lambda create-function-url-config \
  --function-name rust-stream-bedrock-stream \
  --auth-type NONE \
  --cors '{
    "AllowOrigins": ["*"],
    "AllowMethods": ["POST"],
    "AllowHeaders": ["content-type"],
    "MaxAge": 300
  }'

# Obtener la Function URL
FUNCTION_URL=$(aws lambda get-function-url-config \
  --function-name rust-stream-bedrock-stream \
  --query 'FunctionUrl' \
  --output text)

echo "Function URL: $FUNCTION_URL"
```

> **Nota:** `--auth-type NONE` es necesario porque Telegram no soporta autenticacion IAM. La Lambda valida el formato del webhook internamente.

#### 2. Agregar permiso publico a la Function URL

```bash
aws lambda add-permission \
  --function-name rust-stream-bedrock-stream \
  --statement-id telegram-webhook-public \
  --action lambda:InvokeFunctionUrl \
  --principal "*" \
  --function-url-auth-type NONE
```

#### 3. Registrar el webhook en Telegram

```bash
export FUNCTION_URL="<tu-function-url>"
export TELEGRAM_TOKEN="<tu-bot-token>"

# Registrar el webhook
curl -X POST "https://api.telegram.org/bot${TELEGRAM_TOKEN}/setWebhook" \
  -H "Content-Type: application/json" \
  -d "{\"url\": \"${FUNCTION_URL}\"}"

# Verificar el estado del webhook
curl -s "https://api.telegram.org/bot${TELEGRAM_TOKEN}/getWebhookInfo" | jq .
```

#### 4. Configurar el bot token en la Lambda

```bash
# Via SAM deploy
sam deploy --parameter-overrides TelegramBotToken=$TELEGRAM_TOKEN

# O directamente en la Lambda
aws lambda update-function-configuration \
  --function-name rust-stream-bedrock-stream \
  --environment "Variables={BEDROCK_MODEL_ID=us.anthropic.claude-sonnet-4-6,TELEGRAM_BOT_TOKEN=$TELEGRAM_TOKEN}"
```

### Como funciona el streaming en Telegram

A diferencia del flujo directo donde Lambda hace streaming via el channel al cliente, en Telegram el streaming se implementa editando el mensaje progresivamente:

1. **Respuesta inmediata** — Lambda retorna 200 OK al webhook via `channel()` al instante, evitando el timeout de 60s de Telegram.
2. **Procesamiento en background** — `tokio::spawn` ejecuta todo el flujo Bedrock + Telegram sin bloquear la respuesta del webhook.
3. **Placeholder** — Se envia un mensaje con `▍` (cursor) al chat.
4. **Ediciones progresivas** — Cada ~1 segundo se edita el mensaje con el texto acumulado + cursor, dando efecto de "escritura en tiempo real".
5. **Edicion final** — Al completar el stream, se edita con el texto completo sin cursor.

Si la respuesta excede 4096 caracteres (limite de Telegram), se trunca con sufijo `… [truncado]`.

### Eliminar el webhook

```bash
curl -X POST "https://api.telegram.org/bot${TELEGRAM_TOKEN}/deleteWebhook"
```

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

### Prueba del bot de Telegram

```bash
# Enviar un mensaje de prueba directamente a la Lambda
aws lambda invoke \
  --function-name rust-stream-bedrock-stream \
  --cli-binary-format raw-in-base64-out \
  --payload '{
    "body": "{\"update_id\":1,\"message\":{\"message_id\":1,\"from\":{\"id\":123,\"is_bot\":false,\"first_name\":\"Test\"},\"chat\":{\"id\":123,\"type\":\"private\"},\"date\":0,\"text\":\"Hola\"}}"
  }' \
  /tmp/telegram-test.json

cat /tmp/telegram-test.json
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

# Tests de Telegram
cargo test test_telegram
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
| `edit_count` | Numero de ediciones a Telegram durante el stream |
| `placeholder_msg_id` | ID del mensaje de Telegram que se edita progresivamente |

```bash
# Ver logs recientes de la Lambda
sam logs --stack-name rust-stream --tail

# Filtrar por errores
sam logs --stack-name rust-stream --filter "ERROR"

# Filtrar cold starts
sam logs --stack-name rust-stream --filter "cold_start"

# Filtrar mensajes de Telegram
sam logs --stack-name rust-stream --filter "Telegram"

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
|   + detectar formato  |       |   + detectar formato|     |   + detectar formato|
|   + invocar Bedrock   |       |   + invocar Bedrock |     |   + invocar Bedrock |
|   + stream chunks     |       |   + stream chunks   |     |   + stream chunks   |
+-----------------------+       +---------------------+     +---------------------+
      ~100ms init                    ~0ms init                   ~0ms init
```

- **`main()`** corre una sola vez por contenedor: inicializa el `BedrockClient`, el tracing subscriber y el AWS config. Estos se reutilizan en invocaciones posteriores.
- **`handler()`** se ejecuta en cada invocacion pero reutiliza el cliente de Bedrock. Detecta si el body es un `PromptRequest` o un `TelegramUpdate` y enruta al flujo correspondiente.
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
- Para Telegram, el `tokio::spawn` cumple doble funcion: retorna 200 al webhook inmediatamente y mantiene la Lambda viva mientras procesa el stream de Bedrock y edita el mensaje.
- Cold start con Rust + ARM64 (Graviton2) ~ 80-120ms.

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
