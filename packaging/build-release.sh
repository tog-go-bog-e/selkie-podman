#!/bin/sh
# Build a distributable release archive:
#   dist/selkie-<version>-<target>.tar.gz
#
# The archive contains the binary, docs, desktop entry, icons and an installer
# that works from the extracted directory.
set -eu

CRATE=$(cd "$(dirname "$0")/.." && pwd)
cd "$CRATE"

VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
if [ -z "$VERSION" ]; then
    echo "error: could not read version from Cargo.toml" >&2
    exit 1
fi
TARGET=$(rustc -vV | sed -n 's/^host: //p')
NAME="selkie-$VERSION-$TARGET"
STAGE="dist/$NAME"

for size in 16 32 48 64 128 256 512; do
    if [ ! -f "packaging/icons/selkie-$size.png" ]; then
        echo "error: missing icon; run packaging/render-icons.sh" >&2
        exit 1
    fi
done

echo "Building release binary…"
cargo build --release

rm -rf "$STAGE"
mkdir -p "$STAGE"

install -m755 target/release/selkie "$STAGE/"
install -m644 README.md ROADMAP.md "$STAGE/"
install -m755 packaging/install.sh "$STAGE/install.sh"
install -m644 packaging/selkie.desktop "$STAGE/"
cp -R packaging/icons "$STAGE/icons"
for license in LICENSE ../LICENSE; do
    if [ -f "$license" ]; then
        install -m644 "$license" "$STAGE/LICENSE"
        break
    fi
done

tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
if command -v sha256sum >/dev/null 2>&1; then
    (cd dist && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
fi

echo
echo "Built dist/$NAME.tar.gz"
ls -lh "dist/$NAME.tar.gz"
