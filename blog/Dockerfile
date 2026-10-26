# Multi-stage build with a dependency cache (cargo-chef):
#   chef     – toolchain + cargo-chef
#   planner  – computes a "recipe" (the dependency graph) from Cargo.toml/lock
#   builder  – builds ONLY dependencies from the recipe (cached layer), then the app
#   runtime  – slim Debian with the binary, assets/ and config/
#
# Builder and runtime are pinned to the SAME Debian release: the binary links
# against the builder's glibc. (`loco generate deployment docker` emits
# `rust:1.94.0-slim` + `debian:bookworm-slim`; once the rust image's default
# Debian moves past bookworm, that pair fails at runtime with
# "GLIBC_2.xx not found".)
ARG RUST_VERSION=1.97
ARG DEBIAN=bookworm

FROM rust:${RUST_VERSION}-slim-${DEBIAN} AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Dependencies only — this layer is reused until Cargo.toml / Cargo.lock change.
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin blog-cli

FROM debian:${DEBIAN}-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /app --shell /usr/sbin/nologin loco
WORKDIR /app
COPY --from=builder /app/target/release/blog-cli /usr/local/bin/blog-cli
# Templates and static files are read from ./assets at runtime (not embedded);
# mail templates ARE embedded in the binary (include_dir!).
COPY assets ./assets
COPY config ./config
# Uploaded covers live here (local storage driver) — mount a volume on it.
RUN mkdir -p assets/static/uploads && chown -R loco:loco assets/static/uploads
USER loco
ENV LOCO_ENV=production \
    BINDING=0.0.0.0 \
    PORT=5150
EXPOSE 5150
# Exec form: blog-cli is PID 1 and receives SIGTERM from `docker stop`
# directly, so Loco's graceful shutdown runs (in-flight requests finish,
# the queue worker drains its current job).
ENTRYPOINT ["blog-cli"]
CMD ["start"]
