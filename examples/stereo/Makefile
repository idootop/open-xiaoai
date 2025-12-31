build:
	docker run --rm -v $(shell pwd):/app idootop/open-xiaoai-runtime:oh2p \
	cargo build --target armv7-unknown-linux-gnueabihf --release

# 部署到小爱音箱（调试用）
deploy:
	dd if=target/armv7-unknown-linux-gnueabihf/release/stereo \
	| sshpass -p open-xiaoai ssh -o HostKeyAlgorithms=+ssh-rsa root@192.168.31.153 "dd of=/data/stereo"
	dd if=target/armv7-unknown-linux-gnueabihf/release/stereo \
	| sshpass -p open-xiaoai ssh -o HostKeyAlgorithms=+ssh-rsa root@192.168.31.235 "dd of=/data/stereo"