#!/usr/bin/env bash
set -euo pipefail

# ─── Obtener región y nombre del stack desde samconfig.toml ───────────────────
AWS_REGION=$(grep '^region' samconfig.toml | awk -F'"' '{print $2}')
STACK_NAME=$(grep '^stack_name' samconfig.toml | awk -F'"' '{print $2}')

echo "Region: $AWS_REGION"
echo "Stack:  $STACK_NAME"

# ─── Obtener outputs del stack de CloudFormation ──────────────────────────────
FUNCTION_ARN=$(aws cloudformation describe-stacks \
  --stack-name "$STACK_NAME" \
  --region "$AWS_REGION" \
  --query "Stacks[0].Outputs[?OutputKey=='FunctionArn'].OutputValue" \
  --output text)

ROLE_ARN=$(aws cloudformation describe-stacks \
  --stack-name "$STACK_NAME" \
  --region "$AWS_REGION" \
  --query "Stacks[0].Outputs[?OutputKey=='ApiGatewayLambdaRoleArn'].OutputValue" \
  --output text)

echo "FunctionArn: $FUNCTION_ARN"
echo "RoleArn:     $ROLE_ARN"

# ─── Construir URI de integración Lambda streaming ────────────────────────────
LAMBDA_URI="arn:aws:apigateway:${AWS_REGION}:lambda:path/2021-11-15/functions/${FUNCTION_ARN}/response-streaming-invocations"

# ─── Generar spec temporal con valores reales ─────────────────────────────────
sed -e "s|REPLACE_ME_1|${LAMBDA_URI}|" \
    -e "s|REPLACE_ME_2|${ROLE_ARN}|" \
    ApiSpec.yml > temp_spec.yml

echo "temp_spec.yml generado"

# ─── Importar REST API ────────────────────────────────────────────────────────
export API_ID=$(aws apigateway import-rest-api \
  --body 'fileb://temp_spec.yml' \
  --parameters endpointConfigurationTypes=REGIONAL \
  --region "$AWS_REGION" \
  --query 'id' --output text)

echo "API_ID: $API_ID"

# ─── Limpiar archivo temporal ─────────────────────────────────────────────────
rm -f temp_spec.yml

# ─── Crear deployment en stage prod ───────────────────────────────────────────
aws apigateway create-deployment \
  --rest-api-id "$API_ID" \
  --stage-name prod \
  --region "$AWS_REGION"

echo "Listo. API desplegada en: https://${API_ID}.execute-api.${AWS_REGION}.amazonaws.com/prod"
