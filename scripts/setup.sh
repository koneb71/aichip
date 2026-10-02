#!/usr/bin/env bash
#
# Install what Eren needs to build and run, on macOS or Linux.
#
#   ./scripts/setup.sh                  # ask before anything is installed
#   ./scripts/setup.sh --yes            # don't ask
#   ./scripts/setup.sh --dry-run        # print the commands, run none of them
#   ./scripts/setup.sh --with-optional  # also install gh
#
# What it installs is what the README's "Requirements" section lists: git, a
# Rust toolchain (and, on Linux, a C toolchain, pkg-config and the OpenSSL
# headers), Node 22 and pnpm 10 — the versions CI pins — and, if no agent CLI
# is on PATH yet, Claude Code. Anything already present at a good enough
# version is left alone, so running it twice changes nothing the second time.
#
# It never logs anything in: an agent CLI needs its own login, done by running
# it once, and Eren never touches its credentials.

set -euo pipefail

# Arrays are expanded as ${a[@]+"${a[@]}"} throughout: macOS still ships
# bash 3.2, where an empty array under `set -u` is an "unbound variable".

NODE_MAJOR=22
PNPM_MAJOR=10
AGENT_CLIS=(claude opencode codex gemini cursor-agent qwen amp)
# Asking `pnpm --version` through corepack must not stop to ask about a download.
export COREPACK_ENABLE_DOWNLOAD_PROMPT=0

YES=0
DRY_RUN=0
WITH_OPTIONAL=0

usage() {
    sed -n '3,17p' "$0" | sed 's/^# \{0,1\}//'
}

for arg in "$@"; do
    case "$arg" in
        -y | --yes) YES=1 ;;
        -n | --dry-run) DRY_RUN=1 ;;
        --with-optional) WITH_OPTIONAL=1 ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option: $arg (try --help)" >&2
            exit 2
            ;;
    esac
done

# ── Output ───────────────────────────────────────────────────────────────────

if [ -t 1 ]; then
    GREEN=$'\033[32m' YELLOW=$'\033[33m' RED=$'\033[31m' BOLD=$'\033[1m' RESET=$'\033[0m'
else
    GREEN='' YELLOW='' RED='' BOLD='' RESET=''
fi

SUMMARY=()
ok() { echo "${GREEN}✓${RESET} $*"; }
doing() { echo "${YELLOW}→${RESET} $*"; }
fail() {
    echo "${RED}✗${RESET} $*" >&2
    exit 1
}
note() { SUMMARY+=("$1"); }

# Run a command, or only show it under --dry-run.
run() {
    if [ "$DRY_RUN" = 1 ]; then
        echo "  would run: $*"
    else
        echo "  \$ $*"
        "$@"
    fi
}

# The same, through a shell, for the official installers that are piped in.
run_sh() {
    if [ "$DRY_RUN" = 1 ]; then
        echo "  would run: $1"
    else
        echo "  \$ $1"
        bash -c "$1"
    fi
}

confirm() {
    [ "$YES" = 1 ] && return 0
    [ "$DRY_RUN" = 1 ] && return 0
    local answer
    read -r -p "$1 [Y/n] " answer </dev/tty || return 1
    case "$answer" in "" | y | Y | yes | YES) return 0 ;; *) return 1 ;; esac
}

have() { command -v "$1" >/dev/null 2>&1; }

# The major version in something like "v22.11.0" or "10.33.0".
major() { sed -E 's/^[^0-9]*([0-9]+).*/\1/' <<<"$1"; }

# ── Platform ─────────────────────────────────────────────────────────────────

SUDO=()
if [ "$(id -u)" != 0 ]; then
    have sudo || fail "this needs root for the system packages, and sudo is not installed"
    SUDO=(sudo)
fi

OS="$(uname -s)"
case "$OS" in
    Darwin)
        PM=brew
        if ! have brew; then
            fail "Homebrew is needed on macOS and is not installed. Install it with:
    /bin/bash -c \"\$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"
  then run this script again."
        fi
        SUDO=()  # Homebrew refuses to run as root, and needs no sudo.
        ;;
    Linux)
        if have apt-get; then
            PM=apt
        elif have dnf; then
            PM=dnf
        elif have pacman; then
            PM=pacman
        elif have zypper; then
            PM=zypper
        else
            fail "no apt-get, dnf, pacman or zypper here. Install these yourself, then run 'eren doctor':
    git, a C toolchain, pkg-config, the OpenSSL development headers,
    Rust (https://rustup.rs), Node ${NODE_MAJOR}+ and pnpm ${PNPM_MAJOR}"
        fi
        ;;
    *)
        fail "Eren runs on macOS and Linux; this is $OS"
        ;;
esac
ok "${BOLD}$OS${RESET}, installing with $PM"

APT_UPDATED=0
install_pkgs() {
    case "$PM" in
        brew) run brew install "$@" ;;
        apt)
            if [ "$APT_UPDATED" = 0 ]; then
                run ${SUDO[@]+"${SUDO[@]}"} apt-get update
                APT_UPDATED=1
            fi
            run ${SUDO[@]+"${SUDO[@]}"} env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@"
            ;;
        dnf) run ${SUDO[@]+"${SUDO[@]}"} dnf install -y "$@" ;;
        pacman) run ${SUDO[@]+"${SUDO[@]}"} pacman -S --needed --noconfirm "$@" ;;
        zypper) run ${SUDO[@]+"${SUDO[@]}"} zypper --non-interactive install "$@" ;;
    esac
}

# ── git ──────────────────────────────────────────────────────────────────────

if have git; then
    ok "git: $(git --version | sed 's/git version //')"
    note "git          present"
else
    doing "git: installing"
    install_pkgs git
    note "git          installed"
fi

# ── What Rust needs to build Eren ────────────────────────────────────────────

case "$PM" in
    brew)
        if xcode-select -p >/dev/null 2>&1; then
            ok "Xcode Command Line Tools: present"
            note "build tools  present"
        else
            doing "Xcode Command Line Tools: installing"
            run xcode-select --install
            [ "$DRY_RUN" = 1 ] || fail "finish the Command Line Tools install in the window that opened, then run this script again"
        fi
        ;;
    *)
        case "$PM" in
            apt) build_pkgs=(build-essential pkg-config libssl-dev ca-certificates curl) ;;
            dnf) build_pkgs=(gcc make pkgconf-pkg-config openssl-devel ca-certificates curl) ;;
            pacman) build_pkgs=(base-devel pkgconf openssl ca-certificates curl) ;;
            zypper) build_pkgs=(gcc make pkg-config libopenssl-devel ca-certificates curl) ;;
        esac
        # Package managers skip what is already installed, so this is asked
        # every time rather than guessed at from which commands exist.
        doing "build tools: ${build_pkgs[*]}"
        install_pkgs "${build_pkgs[@]}"
        note "build tools  ensured"
        ;;
esac

# ── Rust ─────────────────────────────────────────────────────────────────────

# rustup's default location, so a toolchain installed by an earlier run is
# found even from a shell that has not sourced it yet.
# shellcheck source=/dev/null
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

if have cargo; then
    ok "Rust: $(cargo --version)"
    note "rust         present"
elif confirm "Rust is not installed. Install it with rustup (https://rustup.rs)?"; then
    doing "Rust: installing with rustup"
    run_sh "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal"
    # shellcheck source=/dev/null
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
    note "rust         installed (open a new shell, or: . \"\$HOME/.cargo/env\")"
else
    note "rust         MISSING — install it from https://rustup.rs"
fi

# ── Node ─────────────────────────────────────────────────────────────────────

node_ok() { have node && [ "$(major "$(node --version)")" -ge "$NODE_MAJOR" ]; }

if node_ok; then
    ok "Node: $(node --version)"
    note "node         present"
else
    if have node; then
        doing "Node: $(node --version) is older than ${NODE_MAJOR}; installing ${NODE_MAJOR}"
    else
        doing "Node: installing ${NODE_MAJOR}"
    fi
    case "$PM" in
        brew)
            install_pkgs "node@${NODE_MAJOR}"
            run brew link --overwrite --force "node@${NODE_MAJOR}"
            ;;
        apt)
            # NodeSource, as the Dockerfile does: the distribution's own Node
            # is often older than CI's.
            run_sh "curl -fsSL https://deb.nodesource.com/setup_${NODE_MAJOR}.x | ${SUDO[*]:-} bash -"
            install_pkgs nodejs
            ;;
        dnf)
            run_sh "curl -fsSL https://rpm.nodesource.com/setup_${NODE_MAJOR}.x | ${SUDO[*]:-} bash -"
            install_pkgs nodejs
            ;;
        pacman) install_pkgs nodejs npm ;;
        zypper) install_pkgs "nodejs${NODE_MAJOR}" "npm${NODE_MAJOR}" ;;
    esac
    hash -r
    note "node         installed"
fi

# ── pnpm ─────────────────────────────────────────────────────────────────────

pnpm_ok() { have pnpm && [ "$(major "$(pnpm --version)")" = "$PNPM_MAJOR" ]; }

# Global npm installs need root when Node lives in a root-owned prefix
# (/usr, /usr/local); not with Homebrew, nvm or a home-directory prefix.
node_sudo=()
if [ "${#SUDO[@]}" -gt 0 ] && have npm; then
    prefix="$(npm prefix -g 2>/dev/null || echo /usr)"
    [ -w "$prefix" ] || node_sudo=("${SUDO[@]}")
fi

if pnpm_ok; then
    ok "pnpm: $(pnpm --version)"
    note "pnpm         present"
else
    doing "pnpm: installing ${PNPM_MAJOR}"
    if have corepack; then
        run ${node_sudo[@]+"${node_sudo[@]}"} corepack enable pnpm
        run corepack prepare "pnpm@${PNPM_MAJOR}" --activate
    else
        run ${node_sudo[@]+"${node_sudo[@]}"} npm install -g "pnpm@${PNPM_MAJOR}"
    fi
    hash -r
    note "pnpm         installed"
fi

# ── An agent CLI ─────────────────────────────────────────────────────────────

found=()
for cli in "${AGENT_CLIS[@]}"; do
    have "$cli" && found+=("$cli")
done

if [ "${#found[@]}" -gt 0 ]; then
    ok "agent CLIs: ${found[*]}"
    note "agent CLI    ${found[*]}"
elif confirm "No agent CLI is on PATH. Install Claude Code?"; then
    doing "Claude Code: installing"
    run ${node_sudo[@]+"${node_sudo[@]}"} npm install -g @anthropic-ai/claude-code
    note "agent CLI    claude installed — run 'claude' once to log in"
else
    note "agent CLI    MISSING — Eren needs one: ${AGENT_CLIS[*]}"
fi

# ── Optional ─────────────────────────────────────────────────────────────────

if have gh; then
    ok "gh: $(gh --version 2>/dev/null | head -1 || true)"
    note "gh           present"
elif [ "$WITH_OPTIONAL" = 1 ]; then
    doing "gh: installing"
    case "$PM" in
        apt)
            # GitHub's own repository: the distribution's package lags.
            run_sh "curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | ${SUDO[*]:-} tee /usr/share/keyrings/githubcli-archive-keyring.gpg >/dev/null"
            run_sh "echo \"deb [arch=\$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\" | ${SUDO[*]:-} tee /etc/apt/sources.list.d/github-cli.list >/dev/null"
            APT_UPDATED=0
            install_pkgs gh
            ;;
        dnf) install_pkgs gh ;;
        pacman) install_pkgs github-cli ;;
        *) install_pkgs gh ;;
    esac
    note "gh           installed — run 'gh auth login'"
else
    note "gh           not installed (optional; --with-optional installs it)"
fi

if have docker; then
    note "docker       present"
else
    # Not installed here: on macOS it is a desktop app, and on Linux it means
    # a new repository and a group change — not something to do silently.
    note "docker       not installed (optional, for previews: https://docs.docker.com/get-docker/)"
fi

# ── Summary ──────────────────────────────────────────────────────────────────

echo
if [ "$DRY_RUN" = 1 ]; then
    echo "${BOLD}Summary${RESET} (dry run: nothing was changed)"
else
    echo "${BOLD}Summary${RESET}"
fi
for line in "${SUMMARY[@]}"; do
    echo "  $line"
done
echo
echo "${BOLD}Next${RESET}, from the repository root:"
echo "  cd web && pnpm install && pnpm build && cd ..   # the dashboard"
echo "  cargo run -p eren-cli -- doctor                  # checks every engine"
echo "  cargo run -p eren-cli -- serve                   # http://127.0.0.1:4820"
