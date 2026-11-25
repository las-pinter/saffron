# syntax=docker/dockerfile:1

# 1. Web bundle.
FROM node:22-bookworm-slim@sha256:be979e5523c76fa4d5ca4ebd3099078f220aa47ec39c48ad5b5df9314debd6eb AS web
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable
WORKDIR /src/web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# 2. Server binary with the bundle embedded.
FROM rust:1.99-bookworm@sha256:e6f2cff35d0f309acf8433b4f05878ed77092602c5065cc81801177da0bb3a9c AS server
WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY --from=web /src/web/dist web/dist
RUN cargo build --release --locked -p saffron-server
# The runtime image has no mkdir; create the data directory here for the non-root user.
RUN mkdir -p /data-dir

# 3. Runtime: no shell, no package manager, non-root user (uid 65532).
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=server /src/target/release/saffron /usr/local/bin/saffron
COPY --from=server --chown=65532:65532 /data-dir /data
ENV SAFFRON_BIND=0.0.0.0:8640 \
    SAFFRON_DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 8640
USER nonroot:nonroot
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/saffron", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/saffron"]
