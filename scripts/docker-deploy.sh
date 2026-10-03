#!/usr/bin/env bash
#
# Run Eren and its Postgres with Docker Compose from a published image — the
# one scripts/docker-publish.sh pushed — instead of building from source.
#
#   ./scripts/docker-deploy.sh                  # pull EREN_IMAGE:latest and (re)start
#   ./scripts/docker-deploy.sh --tag 1a2b3c4    # a specific build (roll back the same way)
#   ./scripts/docker-deploy.sh --with-storage   # also object storage for KB attachments
#   ./scripts/docker-deploy.sh --down           # stop it (volumes, and your data, are kept)
#
# It needs only docker-compose.yml, .env and this script, laid out as in the
# repository — a server needs no source and no toolchain. In .env:
#
#   EREN_IMAGE=you/eren                # what docker-publish.sh pushed
#   CLAUDE_CODE_OAUTH_TOKEN=…          # from `claude setup-token`
#   EREN_PROJECTS_DIR=/home/you/code   # the code agents work on
#
# To deploy to another machine from this one, point Docker at it first:
#   DOCKER_HOST=ssh://you@server ./scripts/docker-deploy.sh
# (.env is read here; EREN_PROJECTS_DIR is then a path on the server.)

set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
    sed -n '3,21p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
    echo "✗ $*" >&2
    exit 1
}

# A setting from the environment, else from .env. Read as a value, never
# sourced: .env holds a credential and is not a script.
setting() {
    local name="$1"
    if [ -n "${!name:-}" ]; then
        printf '%s' "${!name}"
    elif [ -f .env ]; then
        sed -n "s/^[[:space:]]*$name=//p" .env | tail -n 1 | sed -e 's/^["'\'']//' -e 's/["'\'']$//'
    fi
}

PROFILES=(--profile app)
DOWN=0
TAG=""

while [ $# -gt 0 ]; do
    case "$1" in
        --tag) TAG="${2:?--tag needs a value}"; shift 2 ;;
        --with-storage) PROFILES+=(--profile storage); shift ;;
        --down) DOWN=1; shift ;;
        -h | --help) usage; exit 0 ;;
        *) fail "unknown option: $1 (try --help)" ;;
    esac
done

command -v docker >/dev/null 2>&1 || fail "docker is not installed"
docker compose version >/dev/null 2>&1 || fail "docker compose (v2) is not available"
[ -f docker-compose.yml ] || fail "docker-compose.yml is not next to scripts/ ($(pwd))"

if [ "$DOWN" = 1 ]; then
    # Every profile, so a storage container started earlier stops too.
    docker compose --profile app --profile storage down
    exit 0
fi

IMAGE="$(setting EREN_IMAGE)"
[ -n "$IMAGE" ] || fail "set EREN_IMAGE (in .env or the environment) to the image docker-publish.sh pushed, e.g. you/eren"
export EREN_IMAGE="$IMAGE"
export EREN_TAG="${TAG:-$(setting EREN_TAG)}"
EREN_TAG="${EREN_TAG:-latest}"

# Checked for presence only; the token's value is never printed.
[ -n "$(setting CLAUDE_CODE_OAUTH_TOKEN)" ] ||
    echo "! CLAUDE_CODE_OAUTH_TOKEN is not set: Eren will start, and every Claude Code run will report \"not logged in\". Run \`claude setup-token\` and put it in .env."
[ -n "$(setting EREN_PROJECTS_DIR)" ] ||
    echo "! EREN_PROJECTS_DIR is not set: agents can only see /workspace inside the container."

echo "→ pulling $EREN_IMAGE:$EREN_TAG"
docker compose "${PROFILES[@]}" pull

echo "→ starting"
# --no-build: use what was pulled, never fall back to building from source.
docker compose "${PROFILES[@]}" up -d --no-build --wait

PORT="$(setting EREN_PORT)"
echo "✓ Eren $EREN_IMAGE:$EREN_TAG is running on http://127.0.0.1:${PORT:-4820}"
echo "  logs: docker compose logs -f eren"
