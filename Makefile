# Makefile para SAM BuildMethod: makefile
# Optimizado para AWS Graviton4 (Neoverse V2 / aarch64)
#
# SAM invoca:  make build-BedrockStreamFunction
# El artefacto debe quedar en $(ARTIFACTS_DIR)/bootstrap

ARTIFACTS_DIR ?= ./target/lambda
TARGET        := aarch64-unknown-linux-gnu
BINARY        := bootstrap

# ─── Build target invocado por sam build ─────────────────────────────────────

.PHONY: build-BedrockStreamFunction
build-BedrockStreamFunction:
	cargo build --release --bin $(BINARY) --target $(TARGET)
	mkdir -p $(ARTIFACTS_DIR)
	cp target/$(TARGET)/release/$(BINARY) $(ARTIFACTS_DIR)/$(BINARY)
