FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
# curl — только ради healthcheck; ca-certificates — для https-трекеров
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/torrent-service /app/torrent-service
COPY static /app/static
WORKDIR /app
USER 65534
EXPOSE 8280 6882 6882/udp
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
  CMD curl -sf http://127.0.0.1:8280/health || exit 1
ENTRYPOINT ["/app/torrent-service"]
