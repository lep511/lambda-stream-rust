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

# Archivos temporales para historial y respuesta
HISTORY=$(mktemp /tmp/chat-history-XXXXXX.json)
RESPONSE_FILE=$(mktemp /tmp/chat-response-XXXXXX.txt)
echo '[]' > "$HISTORY"
trap 'rm -f "$HISTORY" "$RESPONSE_FILE" "$HISTORY.payload"' EXIT

while true; do
  echo ""
  printf "Tu: "
  read -r prompt

  if [ -z "$prompt" ]; then
    continue
  fi

  # Agregar mensaje del usuario y construir payload
  python3 -c "
import json, sys

with open('$HISTORY') as f:
    msgs = json.load(f)

msgs.append({'role': 'user', 'content': sys.argv[1]})

with open('$HISTORY', 'w') as f:
    json.dump(msgs, f)

with open('$HISTORY.payload', 'w') as f:
    json.dump({'messages': msgs}, f)
" "$prompt"

  # Stream en tiempo real + capturar respuesta en archivo
  printf "Claude: "
  curl -s --no-buffer \
    -X POST \
    -H "Content-Type: application/json" \
    -d @"$HISTORY.payload" \
    "$API_URL" | tee "$RESPONSE_FILE"

  echo ""

  # Agregar respuesta del asistente al historial
  python3 -c "
import json, sys

with open('$HISTORY') as f:
    msgs = json.load(f)

with open(sys.argv[1]) as f:
    response = f.read()

msgs.append({'role': 'assistant', 'content': response})

with open('$HISTORY', 'w') as f:
    json.dump(msgs, f)
" "$RESPONSE_FILE"

  rm -f "$HISTORY.payload"
done
