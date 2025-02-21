FROM rust:1.84.1-slim-bullseye AS builder
WORKDIR /usr/src/yellowstone-grpc-kafka
RUN apt-get update && apt-get install -y build-essential gettext-base git libsasl2-dev libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
WORKDIR /usr/src/yellowstone-grpc-kafka
RUN apt-get update && apt-get install -y build-essential gettext-base git libsasl2-dev libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/yellowstone-grpc-kafka/target/release/grpc-kafka /usr/local/bin/grpc-kafka
COPY config-* .
