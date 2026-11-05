# UNVERIFIED: statically reviewed, never built (no Docker on the dev box).
# Multi-stage: web build -> rust build -> slim runtime as non-root uid 10001.
FROM node:22-bookworm-slim AS web
WORKDIR /src/frontend
RUN corepack enable pnpm
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
COPY locales/ /src/locales/
RUN pnpm run build

FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/
# Default translations are embedded into pelty-server (first-run bootstrap).
COPY locales/ locales/
COPY extensions/ extensions/
# Workspace member (developer commands); only needed so the workspace resolves.
COPY xtask/ xtask/
COPY Makefile ./
RUN cargo build --release --locked -p pelty-server -p pelty-docs \
 && ./target/release/pelty-docs /src/target/docs

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 pelty \
 && mkdir /data && chown pelty:pelty /data
COPY --from=build /src/target/release/pelty-server /usr/local/bin/pelty-server
COPY --from=web /src/frontend/dist /app/frontend/dist
COPY --from=build /src/target/docs /app/docs
COPY locales/ /app/locales/
COPY extensions/themes/ /app/themes/
COPY extensions/plugins/ /app/plugins/
RUN chown -R root:root /app && chmod -R a-w /app
# /app is read-only. The working folder /data is writable state: the first start
# writes /data/.env (with a generated PELTY_ENCRYPTION_KEY), /data/eula.txt,
# installed addons (/data/data/addons) and backups (/data/backups).
# Accept the EULA (EULA.md) with PELTY_EULA=true or eula=true in /data/eula.txt;
# it is deliberately not baked in here. Variables set on the container win over /data/.env.
WORKDIR /data
VOLUME /data
ENV PELTY_BIND=0.0.0.0:8080 \
    PELTY_FRONTEND_DIR=/app/frontend/dist \
    PELTY_LOCALES_DIR=/app/locales \
    PELTY_THEMES_DIR=/app/themes \
    PELTY_PLUGINS_DIR=/app/plugins \
    PELTY_DOCS_DIR=/app/docs \
    PELTY_LOG_JSON=true
USER pelty
EXPOSE 8080
# /dev/tcp is a bash feature; debian's /bin/sh (dash) does not support it.
HEALTHCHECK --interval=30s --timeout=3s --start-period=20s --retries=3 CMD ["/bin/bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/8080 && printf 'GET /healthz HTTP/1.0\\r\\n\\r\\n' >&3 && grep -q ok <&3"]
ENTRYPOINT ["pelty-server"]
CMD ["serve"]
