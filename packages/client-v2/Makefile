build-server:
	cargo build --release --bin server --features app

build-client:
	docker run --rm -v $(shell pwd):/app idootop/open-xiaoai-runtime:oh2p \
	cargo build --target armv7-unknown-linux-gnueabihf --release --bin client --features app

run-server:
	cargo run --bin server --features app

# 部署到小爱音箱（调试自用）
deploy:
	dd if=target/armv7-unknown-linux-gnueabihf/release/client \
	| sshpass -p open-xiaoai ssh -o HostKeyAlgorithms=+ssh-rsa root@192.168.31.153 "dd of=/data/client"
	dd if=target/armv7-unknown-linux-gnueabihf/release/client \
	| sshpass -p open-xiaoai ssh -o HostKeyAlgorithms=+ssh-rsa root@192.168.31.235 "dd of=/data/client"