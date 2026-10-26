#!/usr/bin/env bash
# Install the daemon (pre-built release binary, or built from source if none is available),
# enable the systemd user service and copy the GNOME extension.
# Does not use sudo: the udev rule is optional and printed at the end.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="DenizSAHIN570/AstroA50Battery"
BINARY_NAME="astro-battery"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[+]${NC} $*"; }
warn()  { echo -e "${YELLOW}[!]${NC} $*"; }
error() { echo -e "${RED}[x]${NC} $*" >&2; exit 1; }

# ── 1. Get the daemon binary ────────────────────────────────────────────────

STAGED="$(mktemp)"
trap 'rm -f "$STAGED"' EXIT

fetch_release_binary() {
    info "Downloading $BINARY_NAME from GitHub Releases…"
    curl -fsSL "https://github.com/$REPO/releases/latest/download/$BINARY_NAME" -o "$STAGED"
}

build_from_source() {
    local cargo
    cargo="$(command -v cargo 2>/dev/null || true)"
    [[ -z "$cargo" && -x "$HOME/.cargo/bin/cargo" ]] && cargo="$HOME/.cargo/bin/cargo"
    [[ -z "$cargo" ]] && error "cargo not found. Install Rust from https://rustup.rs then re-run."
    info "Building $BINARY_NAME from source…"
    "$cargo" build --release --manifest-path "$HERE/daemon/Cargo.toml"
    cp "$HERE/daemon/target/release/$BINARY_NAME" "$STAGED"
}

if ! fetch_release_binary 2>/dev/null; then
    warn "No pre-built release found — building from source."
    build_from_source
fi

# Stop a running instance first so the binary can be replaced.
systemctl --user stop "$BINARY_NAME.service" 2>/dev/null || true
install -Dm755 "$STAGED" "$HOME/.local/bin/$BINARY_NAME"
info "Daemon installed to ~/.local/bin/$BINARY_NAME"

# ── 2. systemd user service ─────────────────────────────────────────────────

install -Dm644 "$HERE/systemd/$BINARY_NAME.service" "$HOME/.config/systemd/user/$BINARY_NAME.service"
systemctl --user daemon-reload
systemctl --user enable --now "$BINARY_NAME.service"
info "Service enabled and started."

# ── 3. GNOME extension ──────────────────────────────────────────────────────

EXT_UUID="astro-a50-battery@gnome"
EXT_DIR="$HOME/.local/share/gnome-shell/extensions/$EXT_UUID"
mkdir -p "$EXT_DIR"
cp "$HERE"/gnome-extension/{extension.js,format.js,metadata.json,stylesheet.css} "$EXT_DIR/"
info "Extension installed to $EXT_DIR"
warn "Wayland: log out and back in to load it, then run: gnome-extensions enable $EXT_UUID"

sleep 1
systemctl --user --no-pager status "$BINARY_NAME.service" | head -5 || true

if ! [[ -f /etc/udev/rules.d/99-astro-battery.rules ]]; then
    warn "If the daemon cannot open the headset (permission denied), install the udev rule:"
    echo "    sudo install -Dm644 $HERE/udev/99-astro-battery.rules /etc/udev/rules.d/99-astro-battery.rules && sudo udevadm control --reload && sudo udevadm trigger"
fi
