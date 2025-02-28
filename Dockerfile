# Use debian bullseye for ARM64 compatibility
FROM --platform=$BUILDPLATFORM rust:1.85.0-bullseye AS builder
ARG TARGETPLATFORM
ARG BUILDPLATFORM

WORKDIR /usr/src/yellowstone-grpc-kafka

# Install dependencies with specific architecture handling
RUN dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install -y \
    build-essential \
    gettext-base \
    git \
    libsasl2-dev \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release --target aarch64-unknown-linux-gnu

# Use non-slim image for runtime
FROM --platform=$TARGETPLATFORM debian:bullseye

WORKDIR /usr/src/yellowstone-grpc-kafka

RUN apt-get update && \
    apt-get install -y \
    libsasl2-2 \
    libssl1.1 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/yellowstone-grpc-kafka/target/aarch64-unknown-linux-gnu/release/grpc-kafka /usr/local/bin/grpc-kafka
COPY config-* .
COPY healthcheck.sh .

HEALTHCHECK --interval=30s --timeout=3s --start-period=30s CMD ./healthcheck.sh
