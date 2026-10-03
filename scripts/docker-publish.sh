#!/usr/bin/env bash
#
# Build Eren's image from this checkout and push it to a registry (Docker Hub
# unless the name says otherwise).
#
#   ./scripts/docker-publish.sh                       # neiellcare71/eren
#   ./scripts/docker-publish.sh you/eren
#   ./scripts/docker-publish.sh you/eren --platform linux/amd64,linux/arm64
#   ./scripts/docker-publish.sh you/eren --tag v0.1.0 --no-latest
#   ./scripts/docker-publish.sh you/eren --no-push    # build and load locally only
#
#   --platform LIST  what to build for (default: the Docker daemon's own arch).
#                    Name the server's arch when it differs from this machine's;
#                    a foreign arch is built under emulation and takes far longer.
#   --tag TAG        an extra tag (repeatable). The commit's short hash is always one.
#   --no-latest      don't also move :latest.
#   --no-push        build into the local Docker instead (one platform only).
#   --uid / --gid    the user the image runs as (default 1000:1000), for a server
#                    where your user has another id and the mounted code is yours.
#   --claude-code V  the Claude Code version to install (default: the Dockerfile's
#                    pin). "latest" takes npm's newest; the models the dashboard
#                    calls "latest" are whatever this CLI knows.
#
# The image name can also come from EREN_IMAGE (in the environment or .env),
# and defaults to neiellcare71/eren. A name with no namespace (plain `eren`) is
# one no registry would take, so it is built into the local Docker instead.
# It never logs in: run `docker login` yourself first. Nothing secret goes into
# the image — .dockerignore keeps .env out of the build context.

set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
    sed -n '3,28p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
    echo "✗ $*" >&2
    exit 1
}

# EREN_IMAGE from .env, if it is set there and not already in the environment.
# Read as a value, never sourced: .env holds a credential and is not a script.
env_file_value() {
    [ -f .env ] || return 0
    sed -n "s/^[[:space:]]*$1=//p" .env | tail -n 1 | sed -e 's/^["'\'']//' -e 's/["'\'']$//'
}

IMAGE="${EREN_IMAGE:-$(env_file_value EREN_IMAGE)}"
PLATFORM=""
TAGS=()
LATEST=1
PUSH=1
UID_ARG=1000
GID_ARG=1000
CLAUDE_CODE=""

while [ $# -gt 0 ]; do
    case "$1" in
        --platform) PLATFORM="${2:?--platform needs a value}"; shift 2 ;;
        --tag) TAGS+=("${2:?--tag needs a value}"); shift 2 ;;
        --no-latest) LATEST=0; shift ;;
        --no-push) PUSH=0; shift ;;
        --uid) UID_ARG="${2:?--uid needs a value}"; shift 2 ;;
        --gid) GID_ARG="${2:?--gid needs a value}"; shift 2 ;;
        --claude-code) CLAUDE_CODE="${2:?--claude-code needs a version}"; shift 2 ;;
        -h | --help) usage; exit 0 ;;
        -*) fail "unknown option: $1 (try --help)" ;;
        *) IMAGE="$1"; shift ;;
    esac
done

IMAGE="${IMAGE:-neiellcare71/eren}"
# A name with no namespace (no `/`) cannot be pushed: Docker Hub would read it
# as an official image. Such an image is for this machine only.
if [ "$PUSH" = 1 ] && [[ "$IMAGE" != */* ]]; then
    echo "→ $IMAGE has no registry namespace (you/eren), so it is built into the local Docker, not pushed"
    PUSH=0
fi
# Only the last path segment can carry a tag; a colon before it is a
# registry's port (registry.local:5000/eren).
case "${IMAGE##*/}" in
    *:*) fail "give the image without a tag ($IMAGE); use --tag for tags" ;;
esac

command -v docker >/dev/null 2>&1 || fail "docker is not installed"
docker buildx version >/dev/null 2>&1 || fail "docker buildx is not available"
docker info >/dev/null 2>&1 || fail "the Docker daemon is not reachable — is Docker running?"

if [ -z "$PLATFORM" ]; then
    PLATFORM="linux/$(docker version --format '{{.Server.Arch}}')"
fi
if [ "$PUSH" = 0 ] && [[ "$PLATFORM" == *,* ]]; then
    fail "--no-push loads into the local Docker, which holds one platform at a time"
fi

REVISION="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
SHORT="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    echo "! the working tree has uncommitted changes; they go into the image, tagged $SHORT-dirty"
    SHORT="$SHORT-dirty"
fi
TAGS=("$SHORT" ${TAGS[@]+"${TAGS[@]}"})
[ "$LATEST" = 1 ] && TAGS+=("latest")

BUILD_ARGS=(--build-arg "UID=$UID_ARG" --build-arg "GID=$GID_ARG")
[ -n "$CLAUDE_CODE" ] && BUILD_ARGS+=(--build-arg "CLAUDE_CODE_VERSION=$CLAUDE_CODE")

TAG_ARGS=()
for t in "${TAGS[@]}"; do
    TAG_ARGS+=(--tag "$IMAGE:$t")
done

# Multi-platform builds and pushes need a builder that is not the plain
# `docker` driver. A dedicated one keeps its cache between runs and leaves
# whatever builder you selected yourself alone.
BUILDER=eren-builder
if ! docker buildx inspect "$BUILDER" >/dev/null 2>&1; then
    echo "→ creating buildx builder $BUILDER"
    docker buildx create --name "$BUILDER" --driver docker-container >/dev/null
fi

if [ "$PUSH" = 1 ]; then
    OUTPUT=--push
else
    OUTPUT=--load
fi

echo "→ building $IMAGE (${TAGS[*]}) for $PLATFORM"
docker buildx build \
    --builder "$BUILDER" \
    --platform "$PLATFORM" \
    "${BUILD_ARGS[@]}" \
    --label "org.opencontainers.image.revision=$REVISION" \
    --label "org.opencontainers.image.created=$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "${TAG_ARGS[@]}" \
    "$OUTPUT" \
    . || {
    [ "$PUSH" = 1 ] && echo "  (a push refused as unauthorized means: run \`docker login\` and try again)" >&2
    exit 1
}

if [ "$PUSH" = 1 ]; then
    echo "✓ pushed $IMAGE:${TAGS[0]}"
    echo "  deploy it with: EREN_IMAGE=$IMAGE EREN_TAG=${TAGS[0]} ./scripts/docker-deploy.sh"
else
    echo "✓ built $IMAGE:${TAGS[0]} into the local Docker"
    echo "  run it with: EREN_IMAGE=$IMAGE EREN_TAG=${TAGS[0]} ./scripts/docker-deploy.sh"
fi
