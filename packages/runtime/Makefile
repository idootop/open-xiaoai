# Makefile for open-xiaoai-runtime

# 使用 host.docker.internal 访问宿主机的代理端口（加速构建）
# 注意：代理软件需要开启 "Allow LAN"（允许局域网连接）
PROXY := http://host.docker.internal:7890
IMAGE_NAME := idootop/open-xiaoai-runtime:oh2p

# 显示帮助信息
help:
	@echo "Available targets:"
	@echo "  make build    - 构建交叉编译环境镜像"
	@echo "  make run-x86  - 进入交叉编译环境（Ubuntu x86_64）"
	@echo "  make run-armv7  - 进入小爱音箱模拟环境（OpenWrt ARMv7）"
	@echo "  make help     - 显示此帮助信息"
	@echo ""
	@echo "Variables:"
	@echo "  PROXY=$(PROXY)"
	@echo "  IMAGE_NAME=$(IMAGE_NAME)"

# 构建交叉编译环境镜像
build:
	@echo "Building Docker image: $(IMAGE_NAME)..."
	docker build \
		--build-arg http_proxy=$(PROXY) \
		--build-arg https_proxy=$(PROXY) \
		-t $(IMAGE_NAME) .

# 进入交叉编译环境（Ubuntu x86_64）
run-x86:
	@echo "Entering cross-compilation environment (Ubuntu x86_64)..."
	docker run --rm -it --privileged $(IMAGE_NAME)

# 进入小爱音箱模拟环境（OpenWrt ARMv7）
run-armv7:
	@echo "Entering XiaoAi speaker simulation environment (OpenWrt ARMv7)..."
	docker run --rm -it --privileged $(IMAGE_NAME) run

