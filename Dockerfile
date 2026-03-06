FROM rust:1.85-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libasound2-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    libvulkan-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .

RUN cargo build --release && strip target/release/claudio


FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libasound2 \
    libxkbcommon0 \
    libvulkan1 \
    mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/*

# Install uv
COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

# Install claudio binary
COPY --from=builder /build/target/release/claudio /usr/local/bin/claudio

# Install ML service
COPY ml_service /opt/claudio/ml_service
WORKDIR /opt/claudio/ml_service
RUN uv sync

WORKDIR /
ENV CLAUDIO_ML_SERVICE_DIR=/opt/claudio/ml_service

ENTRYPOINT ["claudio"]
CMD ["start", "--foreground"]
