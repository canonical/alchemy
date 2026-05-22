FROM rust:bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM ubuntu:latest
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/alchemy /usr/local/bin/alchemy
COPY scripts/check /opt/resource/check
COPY scripts/in /opt/resource/in
COPY scripts/out /opt/resource/out
COPY docker/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod 0755 \
      /usr/local/bin/alchemy \
      /opt/resource/check \
      /opt/resource/in \
      /opt/resource/out \
      /usr/local/bin/docker-entrypoint.sh

WORKDIR /work
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
