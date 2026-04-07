#!/usr/bin/env bash
set -euo pipefail

# ─── Obtener API_ID y región ──────────────────────────────────────────────────
AWS_REGION=$(grep '^region' samconfig.toml | awk -F'"' '{print $2}')

API_ID=$(aws apigateway get-rest-apis \
  --region "$AWS_REGION" \
  --query "items[?name=='APIGW-ResponseStreamingRust'].id" \
  --output text)

API_URL="https://${API_ID}.execute-api.${AWS_REGION}.amazonaws.com/prod/lambda"

echo "API: $API_URL"
echo "Escribe tu mensaje (Ctrl+C para salir)"
echo "=========================================="

# Archivos temporales
HISTORY=$(mktemp /tmp/chat-history-XXXXXX.json)
RESPONSE=$(mktemp /tmp/chat-response-XXXXXX.txt)
echo '[]' > "$HISTORY"
trap 'rm -f "$HISTORY" "$HISTORY.tmp" "$RESPONSE"' EXIT

while true; do
  echo ""
  printf "Tu: "
  read -r prompt

  if [ -z "$prompt" ]; then
    continue
  fi

  # Agregar mensaje del usuario al historial usando jq
  jq --arg msg "$prompt" '. += [{"role": "user", "content": $msg}]' \
    "$HISTORY" > "$HISTORY.tmp" && mv "$HISTORY.tmp" "$HISTORY"

  # Construir payload con el historial completo
  payload=$(jq '{messages: .}' "$HISTORY")

  # Stream en tiempo real + capturar respuesta en archivo
  printf "Claude: "
  curl -s --no-buffer \
    -X POST \
    -H "Content-Type: application/json" \
    -d "$payload" \
    "$API_URL" | tee "$RESPONSE"

  echo ""

  # Agregar respuesta del asistente al historial
  response=$(<"$RESPONSE")
  jq --arg msg "$response" '. += [{"role": "assistant", "content": $msg}]' \
    "$HISTORY" > "$HISTORY.tmp" && mv "$HISTORY.tmp" "$HISTORY"
done
