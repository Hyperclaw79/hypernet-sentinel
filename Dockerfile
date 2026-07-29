# syntax=docker/dockerfile:1.7
FROM rust:1.97.1-bookworm AS builder
WORKDIR /source
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY web ./web
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/source/target \
    cargo build --locked --release -p sentinel-server && \
    cp target/release/sentinel-server /tmp/hypernet-sentinel

FROM debian:bookworm-slim AS runtime
ARG VCS_REF=unknown
ARG VERSION=unknown
ARG SOURCE_URL=https://github.com/Hyperclaw79/hypernet-sentinel
ARG UPSTREAM_REVISION=unknown
LABEL org.opencontainers.image.title="Hypernet Sentinel" \
      org.opencontainers.image.source="$SOURCE_URL" \
      org.opencontainers.image.licenses="GPL-3.0-only" \
      org.opencontainers.image.revision="$VCS_REF" \
      org.opencontainers.image.version="$VERSION" \
      io.hypernet-sentinel.upstream-revision="$UPSTREAM_REVISION"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 sentinel && useradd --uid 10001 --gid sentinel --no-create-home --shell /usr/sbin/nologin sentinel && \
    install -d -o sentinel -g sentinel /data
COPY --from=builder /tmp/hypernet-sentinel /usr/local/bin/hypernet-sentinel
USER 10001:10001
ENV SENTINEL_BIND=0.0.0.0:8080 SENTINEL_DATA_DIR=/data RUST_LOG=sentinel_server=info
EXPOSE 8080
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["/usr/local/bin/hypernet-sentinel","--healthcheck"]
ENTRYPOINT ["/usr/local/bin/hypernet-sentinel"]
