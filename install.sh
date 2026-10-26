#!/usr/bin/env bash
# Build the daemon, install it to ~/.local/bin and enable the systemd user service.
# Does not use sudo: the udev rule is optional and printed at the end.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BINARY_NAME="astro-battery"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[+]${NC} $*"; }
warn()  { echo -e "${YELLOW}[!]${NC} $*"; }
error() { echo -e "${RED}[x]${NC} $*" >&2; exit 1; }

cargo="$(command -v cargo 2>/dev/null || true)"
[[ -z "$cargo" && -x "$HOME/.cargo/bin/cargo" ]] && cargo="$HOME/.cargo/bin/cargo"
[[ -z "$cargo" ]] && error "cargo not found. Install Rust from https://rustup.rs then re-run."

info "Building $BINARY_NAME…"
"$cargo" build --release --manifest-path "$HERE/daemon/Cargo.toml"

# Stop a running instance first so the binary can be replaced.
systemctl --user stop "$BINARY_NAME.service" 2>/dev/null || true
install -Dm755 "$HERE/daemon/target/release/$BINARY_NAME" "$HOME/.local/bin/$BINARY_NAME"
info "Daemon installed to ~/.local/bin/$BINARY_NAME"

install -Dm644 "$HERE/systemd/$BINARY_NAME.service" "$HOME/.config/systemd/user/$BINARY_NAME.service"
systemctl --user daemon-reload
systemctl --user enable --now "$BINARY_NAME.service"
info "Service enabled and started."

EXT_UUID="astro-a50-battery@gnome"
EXT_DIR="$HOME/.local/share/gnome-shell/extensions/$EXT_UUID"
mkdir -p "$EXT_DIR"
cp "$HERE"/gnome-extension/{extension.js,format.js,metadata.json,stylesheet.css} "$EXT_DIR/"
info "Extension installed to $EXT_DIR"
warn "Wayland: log out and back in to load it, then run: gnome-extensions enable $EXT_UUID"

sleep 1
systemctl --user --no-pager status "$BINARY_NAME.service" | head -5 || true

if ! [[ -f /etc/udev/rules.d/99-astro-battery.rules ]]; then
    warn "Optional: install the udev rule so hidraw access doesn't depend on other rules:"
    echo "    sudo install -Dm644 $HERE/udev/99-astro-battery.rules /etc/udev/rules.d/99-astro-battery.rules && sudo udevadm control --reload && sudo udevadm trigger"
fi
