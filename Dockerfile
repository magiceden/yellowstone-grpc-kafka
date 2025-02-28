FROM --platform=$BUILDPLATFORM rust:1.85.0-slim-bullseye AS builder
WORKDIR /usr/src/yellowstone-grpc-kafka
RUN dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install -y build-essential gettext-base git libsasl2-dev libssl-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release --target aarch64-unknown-linux-gnu

FROM --platform=$TARGETPLATFORM rust:1.85.0-slim-bullseye
WORKDIR /usr/src/yellowstone-grpc-kafka
RUN apt-get update && apt-get install -y build-essential gettext-base git libsasl2-dev libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/yellowstone-grpc-kafka/target/aarch64-unknown-linux-gnu/release/grpc-kafka /usr/local/bin/grpc-kafka
COPY config-* .

COPY healthcheck.sh .
HEALTHCHECK --interval=30s --timeout=3s --start-period=30s CMD ./healthcheck.sh
