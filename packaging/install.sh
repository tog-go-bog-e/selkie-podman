#!/bin/sh
# Install Selkie for the current user.
#
# Works both from the source tree (packaging/install.sh) and from a release
# tarball (install.sh at the archive root).
#
# Overrides: PREFIX (default ~/.local), BINDIR, DATADIR, DESTDIR.
# Usage: ./install.sh [--uninstall]
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)

PREFIX=${PREFIX:-$HOME/.local}
BINDIR=${BINDIR:-$PREFIX/bin}
DATADIR=${DATADIR:-$PREFIX/share}
ICONDIR=${ICONDIR:-$DATADIR/icons/hicolor}
APPDIR=${APPDIR:-$DATADIR/applications}
DESTDIR=${DESTDIR:-}

SIZES="16 32 48 64 128 256 512"

# Locate the assets in either layout.
if [ -x "$HERE/selkie" ]; then
    BIN=$HERE/selkie
elif [ -x "$HERE/../target/release/selkie" ]; then
    BIN=$HERE/../target/release/selkie
else
    echo "error: no selkie binary found; run 'cargo build --release' first" >&2
    exit 1
fi

if [ -d "$HERE/icons" ]; then
    ICONS=$HERE/icons
elif [ -d "$HERE/packaging/icons" ]; then
    ICONS=$HERE/packaging/icons
else
    echo "error: no icons/ directory found; run packaging/render-icons.sh first" >&2
    exit 1
fi

if [ -f "$HERE/selkie.desktop" ]; then
    DESKTOP=$HERE/selkie.desktop
elif [ -f "$HERE/packaging/selkie.desktop" ]; then
    DESKTOP=$HERE/packaging/selkie.desktop
else
    echo "error: selkie.desktop not found" >&2
    exit 1
fi

refresh_caches() {
    # Only meaningful for a real (non-staged) install.
    [ -z "$DESTDIR" ] || return 0
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APPDIR" >/dev/null 2>&1 || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f -t "$ICONDIR" >/dev/null 2>&1 || true
    fi
}

uninstall() {
    rm -f "$DESTDIR$BINDIR/selkie"
    rm -f "$DESTDIR$APPDIR/selkie.desktop"
    for size in $SIZES; do
        rm -f "$DESTDIR$ICONDIR/${size}x${size}/apps/selkie.png"
    done
    refresh_caches
    echo "Removed selkie from $DESTDIR$PREFIX"
}

install_all() {
    install -Dm755 "$BIN" "$DESTDIR$BINDIR/selkie"
    install -Dm644 "$DESKTOP" "$DESTDIR$APPDIR/selkie.desktop"
    for size in $SIZES; do
        source_icon="$ICONS/selkie-$size.png"
        if [ ! -f "$source_icon" ]; then
            echo "error: missing $source_icon (run packaging/render-icons.sh)" >&2
            exit 1
        fi
        install -Dm644 "$source_icon" \
            "$DESTDIR$ICONDIR/${size}x${size}/apps/selkie.png"
    done
    refresh_caches

    echo "Installed:"
    echo "  $DESTDIR$BINDIR/selkie"
    echo "  $DESTDIR$APPDIR/selkie.desktop"
    echo "  $DESTDIR$ICONDIR/<size>/apps/selkie.png"
    case ":$PATH:" in
        *":$BINDIR:"*) ;;
        *) echo "note: $BINDIR is not in PATH; add it to launch 'selkie' directly" ;;
    esac
}

case "${1:-}" in
    --uninstall|uninstall)
        uninstall
        ;;
    ""|--install|install)
        install_all
        ;;
    *)
        echo "usage: $0 [--uninstall]" >&2
        exit 2
        ;;
esac
