FROM rust:1-bookworm AS build
WORKDIR /src
# CARGO_FEATURES: "sso" для Kerberos SSO (нужны krb5-заголовки)
ARG CARGO_FEATURES=""
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN if [ -n "$CARGO_FEATURES" ]; then cargo build --release --features "$CARGO_FEATURES"; \
    else cargo build --release; fi

FROM debian:bookworm-slim
# curl — healthcheck; ca-certificates — https-трекеры;
# libgssapi-krb5-2 — SPNEGO/Kerberos (нужен при CARGO_FEATURES=sso, но безопасен и без)
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates libgssapi-krb5-2 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/torrent-service /app/torrent-service
COPY static /app/static
WORKDIR /app
USER 65534
EXPOSE 8280 6882 6882/udp
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
  CMD curl -sf http://127.0.0.1:8280/health || exit 1
ENTRYPOINT ["/app/torrent-service"]
