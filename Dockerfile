# aichip in a container.
#
# Read the "Running in Docker" section of README.md before using this. The
# short version: aichip works by spawning the official `claude` CLI, so the
# container needs both the CLI and a way to authenticate. Inside a container
# there is no keychain and no browser, which leaves exactly one option — a
# long-lived token from `claude setup-token`, supplied as an environment
# variable. That is a real trade-off, not a detail.

# ── 1. Dashboard ───────────────────────────────────────────────────────────
FROM node:22-slim AS web
WORKDIR /web
RUN corepack enable
# Manifests first so a source-only edit doesn't reinstall the world.
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# ── 2. Server ──────────────────────────────────────────────────────────────
# Pinned to the same Debian release as the runtime stage below. A newer
# builder links against a newer glibc and the binary won't start.
FROM rust:1-slim-bookworm AS server
WORKDIR /src
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release --locked -p aichip-cli

# ── 3. Runtime ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim
# git: worktrees are the whole isolation model. node: the CLI ships as an npm
# package. ca-certificates: the CLI talks to Anthropic over TLS.
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates curl gnupg \
    && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g @anthropic-ai/claude-code@2.1.220 \
    && apt-get purge -y --auto-remove curl gnupg \
    && rm -rf /var/lib/apt/lists/*

# Runs as a normal user: an agent with a shell should not be uid 0, and the
# uid is overridable so files it writes into your mounted code stay yours.
ARG UID=1000
ARG GID=1000
RUN groupadd -g "${GID}" aichip 2>/dev/null || true \
    && useradd -m -u "${UID}" -g "${GID}" -s /bin/bash aichip 2>/dev/null || true

COPY --from=server /src/target/release/aichip /usr/local/bin/aichip
COPY --from=web /web/dist /srv/aichip/web

ENV AICHIP_WEB_DIST=/srv/aichip/web
# Bind wide inside the container: the container's own loopback is not the
# host's, so nothing outside the namespace could reach it otherwise. What is
# actually exposed is decided by the port mapping you declare in compose — and
# `-p 4820:4820` publishes on every interface, so it is reachable from your
# network. aichip has no authentication, so bind `127.0.0.1:4820:4820` unless
# you mean to share it.
ENV AICHIP_BIND=0.0.0.0
# Acknowledged here because binding wide is the only way a container can work,
# not because the exposure is smaller. See `aichip_server::exposure`.
ENV AICHIP_TRUST_NETWORK=1
EXPOSE 4820

USER aichip
WORKDIR /home/aichip
ENTRYPOINT ["aichip"]
CMD ["serve", "--headless"]
