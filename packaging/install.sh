#!/bin/sh
# Build Tiramisu Garage and install it for the current user:
# binary in ~/.local/bin, Start Menu entry, and the icon at every size.
set -eu
cd "$(dirname "$0")/.."
cargo build --release
install -Dm755 target/release/tiramisu-garage "$HOME/.local/bin/tiramisu-garage"
icons="$HOME/.local/share/icons/hicolor"
install -Dm644 packaging/tiramisu-garage.svg "$icons/scalable/apps/tiramisu-garage.svg"
if command -v rsvg-convert >/dev/null; then
    for s in 16 22 24 32 48 64 128 256; do
        mkdir -p "$icons/${s}x${s}/apps"
        rsvg-convert -w "$s" -h "$s" packaging/tiramisu-garage.svg -o "$icons/${s}x${s}/apps/tiramisu-garage.png"
    done
fi
# absolute Exec path: a session started from the login screen may not have ~/.local/bin on PATH
sed "s|^Exec=tiramisu-garage|Exec=$HOME/.local/bin/tiramisu-garage|" packaging/tiramisu-garage.desktop \
    > "$HOME/.local/share/applications/tiramisu-garage.desktop"
command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$icons" 2>/dev/null || true
echo "Installed. Find it in the Start Menu as \"Tiramisu Garage\"."
