FROM rust:1.85.0-slim-bullseye AS builder
WORKDIR /usr/src/yellowstone-grpc-kafka
RUN apt-get update && apt-get install -y build-essential gettext-base git libsasl2-dev libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
WORKDIR /usr/src/yellowstone-grpc-kafka
RUN apt-get update && \
    apt-get install -y build-essential gettext-base git libsasl2-dev libssl-dev pkg-config ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    mkdir -p /usr/local/ssl/certs && \
    ln -s /etc/ssl/certs/* /usr/local/ssl/certs/

COPY --from=builder /usr/src/yellowstone-grpc-kafka/target/release/grpc-kafka /usr/local/bin/grpc-kafka
COPY config-* .
COPY healthcheck.sh .

# Create user with same ID as in Kubernetes config
RUN groupadd -g 1001 appuser && \
    useradd -u 1001 -g 1001 -s /bin/bash -m appuser && \
    chown -R appuser:appuser /usr/src/yellowstone-grpc-kafka && \
    chmod +x /usr/src/yellowstone-grpc-kafka/healthcheck.sh

HEALTHCHECK --interval=30s --timeout=3s --start-period=30s CMD ./healthcheck.sh

# Switch to non-root user
USER appuser
