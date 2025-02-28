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
HEALTHCHECK --interval=30s --timeout=3s --start-period=30s CMD ./healthcheck.sh
