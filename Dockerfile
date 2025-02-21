FROM rust:1.84.1-slim-bullseye
WORKDIR /usr/src/yellowstone-grpc-kafka
RUN apt-get update && apt-get install -y build-essential gettext-base git libsasl2-dev libssl-dev pkg-config
COPY . .
