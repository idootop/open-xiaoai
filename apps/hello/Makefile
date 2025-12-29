BIN ?= server 

build-stereo:
	$(MAKE) build-arm32 BIN=stereo

build-client:
	$(MAKE) build-arm32 BIN=client

build-server:
	cargo build --release --bin server

run:
	./target/release/$(BIN)

build-arm32:
	docker run --rm -v $(shell pwd):/app open-xiaoai-runtime \
	cargo build --target armv7-unknown-linux-gnueabihf --release --bin $(BIN)

build-arm64:
	cargo build --release --bin $(BIN)