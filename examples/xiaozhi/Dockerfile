FROM python:3.12-slim AS builder

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# 更新源
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl build-essential libopus-dev \
    && rm -rf /var/lib/apt/lists/*

# 安装 Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# 安装 uv
COPY --from=ghcr.io/astral-sh/uv:0.7 /uv /uvx /bin/

# 设置环境变量
ENV BASH_ENV=/root/.bash_env
RUN touch "$BASH_ENV"
RUN echo '. "$BASH_ENV"' >> "$HOME/.bashrc"
RUN echo '[ -s "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"' >> "$BASH_ENV"

# 设置工作目录
WORKDIR /app

# 复制项目文件
COPY examples/xiaozhi .
COPY packages/client-rust ./client-rust

# 构建
RUN sed -i 's/\.\.\/\.\.\/packages\///g' Cargo.toml \
    && cargo build --release

# 安装依赖
RUN --mount=type=cache,target=/root/.cache/uv \
    uv remove pyaudio && uv sync --locked --no-install-project --no-editable

# 构建 wheel 并安装
RUN uv run maturin build --release && uv remove maturin


FROM python:3.12-slim

WORKDIR /app

# 更新源
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl libopus-dev \
    && rm -rf /var/lib/apt/lists/*

ARG TARGETARCH
ENV CLI=true
ENV LD_LIBRARY_PATH=/usr/local/lib:/app/.venv/lib/python3.12/site-packages/onnxruntime/capi

COPY --from=builder /app/.venv /app/.venv
COPY examples/xiaozhi/main.py . 
COPY examples/xiaozhi/xiaozhi ./xiaozhi

RUN set -eux; \
    case "$TARGETARCH" in \
      arm64) ort_arch="aarch64" ;; \
      amd64) ort_arch="x64" ;; \
      *) echo "Unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -L "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-linux-${ort_arch}-1.23.2.tgz" -o /tmp/onnxruntime.tgz; \
    tar -xzf /tmp/onnxruntime.tgz -C /tmp; \
    cp /tmp/onnxruntime-linux-${ort_arch}-1.23.2/lib/libonnxruntime.so.1.23.2 /usr/local/lib/; \
    ln -sf /usr/local/lib/libonnxruntime.so.1.23.2 /usr/local/lib/libonnxruntime.so; \
    ldconfig; \
    rm -rf /tmp/onnxruntime.tgz /tmp/onnxruntime-linux-${ort_arch}-1.23.2

# 镜像已内置可用的 keywords.txt，启动时不再强制重建
CMD ["/bin/bash", "-c", "source /app/.venv/bin/activate && python main.py"]
