#!/bin/sh
# Build a Debian package:
#   dist/selkie_<version>_<arch>.deb
#
# The package installs the binary as /usr/bin/selkie, the desktop
# entry as /usr/share/applications/selkie.desktop and the icons into
# the hicolor theme, so the app shows up in the desktop menu after:
#
#   sudo apt install ./dist/selkie_<version>_<arch>.deb
#
# Only dpkg-deb is required. When dpkg-shlibdeps (dpkg-dev) is available the
# shared-library dependencies are computed from the binary instead of guessed.
set -eu

CRATE=$(cd "$(dirname "$0")/.." && pwd)
cd "$CRATE"

VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
if [ -z "$VERSION" ]; then
    echo "error: could not read version from Cargo.toml" >&2
    exit 1
fi

if ! command -v dpkg-deb >/dev/null 2>&1; then
    echo "error: dpkg-deb not found; install dpkg or build on a Debian system" >&2
    exit 1
fi

ARCH=$(dpkg --print-architecture)
# Cargo's version is the upstream version; the Debian revision is bumped when the
# packaging itself changes.
DEB_VERSION="${VERSION}-${DEB_REVISION:-1}"
NAME="selkie_${DEB_VERSION}_${ARCH}"
STAGE="dist/deb/$NAME"
OUT="dist/$NAME.deb"
PKG=selkie
DOC="$STAGE/usr/share/doc/$PKG"

# The Maintainer field (and the changelog/copyright) is published inside the
# package. Set DEB_MAINTAINER="Name <email>" to choose it explicitly; otherwise
# the git identity is used, and the result is printed so it is never a surprise.
MAINTAINER_NAME=$(git config --get user.name 2>/dev/null || true)
MAINTAINER_EMAIL=$(git config --get user.email 2>/dev/null || true)
if [ -n "${DEB_MAINTAINER:-}" ]; then
    MAINTAINER=$DEB_MAINTAINER
    MAINTAINER_NAME=${DEB_MAINTAINER%% <*}
elif [ -n "$MAINTAINER_NAME" ] && [ -n "$MAINTAINER_EMAIL" ]; then
    MAINTAINER="$MAINTAINER_NAME <$MAINTAINER_EMAIL>"
else
    MAINTAINER="$PKG maintainers <noreply@example.invalid>"
fi
echo "Maintainer: $MAINTAINER (override with DEB_MAINTAINER)"

for size in 16 32 48 64 128 256 512; do
    if [ ! -f "packaging/icons/selkie-$size.png" ]; then
        echo "error: missing icon; run packaging/render-icons.sh" >&2
        exit 1
    fi
done

echo "Building release binary…"
cargo build --release

# Shared-library dependencies: prefer what the binary actually needs.
SHLIBS=""
if command -v dpkg-shlibdeps >/dev/null 2>&1; then
    TMP=$(mktemp -d)
    trap 'rm -rf "$TMP"' EXIT
    mkdir -p "$TMP/debian"
    printf 'Source: %s\nMaintainer: %s\n\nPackage: %s\nArchitecture: any\nDescription: local build\n local build\n' \
        "$PKG" "$MAINTAINER" "$PKG" > "$TMP/debian/control"
    SHLIBS=$(cd "$TMP" && dpkg-shlibdeps -O -e"$CRATE/target/release/selkie" 2>/dev/null \
        | sed -n 's/^shlibs:Depends=//p')
fi
case "$SHLIBS" in
    *libc6*) ;;
    *) SHLIBS="libc6, libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0" ;;
esac
# libvulkan1 and libwayland-client0 are dlopen()ed by the renderer/backend
# rather than linked, so dpkg-shlibdeps cannot see them.
DEPENDS="$SHLIBS, libvulkan1, libwayland-client0, podman"

echo "Staging the package tree…"
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" "$STAGE/usr/bin" "$STAGE/usr/share/applications" "$DOC"

install -m755 target/release/selkie "$STAGE/usr/bin/"
# Debian wants stripped binaries in the archive; the build artifact is left
# untouched for debugging.
if command -v strip >/dev/null 2>&1; then
    strip --strip-unneeded "$STAGE/usr/bin/selkie"
fi
install -m644 packaging/selkie.desktop "$STAGE/usr/share/applications/"
mkdir -p "$STAGE/usr/share/man/man1"
gzip -9n -c packaging/selkie.1 > "$STAGE/usr/share/man/man1/selkie.1.gz"
chmod 644 "$STAGE/usr/share/man/man1/selkie.1.gz"
for size in 16 32 48 64 128 256 512; do
    install -Dm644 "packaging/icons/selkie-$size.png" \
        "$STAGE/usr/share/icons/hicolor/${size}x${size}/apps/selkie.png"
done
install -m644 README.md ROADMAP.md "$DOC/"

# DEP-5 copyright. Apache-2.0 is a common license on Debian, so point at the
# system copy instead of shipping the full text.
{
    echo "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
    echo "Upstream-Name: $PKG"
    echo "Comment: built locally from the $PKG source tree"
    echo
    echo "Files: *"
    echo "Copyright: $(date +%Y) ${MAINTAINER_NAME:-$PKG maintainers}"
    echo "License: Apache-2.0"
    echo " On Debian systems the full text of the Apache License, Version 2.0 can be"
    echo " found in /usr/share/common-licenses/Apache-2.0."
} > "$DOC/copyright"
chmod 644 "$DOC/copyright"

printf '%s (%s) unstable; urgency=medium\n\n  * Local build from the %s source tree.\n\n -- %s  %s\n' \
    "$PKG" "$DEB_VERSION" "$PKG" "$MAINTAINER" "$(date -R)" \
    | gzip -9n > "$DOC/changelog.Debian.gz"
chmod 644 "$DOC/changelog.Debian.gz"

# dpkg-deb keeps the staging directory's modes, which follow the builder's
# umask.
find "$STAGE" -type d -exec chmod 755 {} +

INSTALLED_SIZE=$(du -sk "$STAGE" | cut -f1)
cat > "$STAGE/DEBIAN/control" <<EOF
Package: $PKG
Version: $DEB_VERSION
Section: admin
Priority: optional
Architecture: $ARCH
Installed-Size: $INSTALLED_SIZE
Maintainer: $MAINTAINER
Depends: $DEPENDS
Recommends: mesa-vulkan-drivers
Description: lightweight Podman Desktop client built with Rust and GPUI
 A small desktop application for the local Podman engine that shells out to
 the podman CLI. It lists containers, images, volumes, networks, pods, compose
 projects, machines and connections, with logs, stats, an interactive terminal,
 a dashboard and the usual resource actions.
EOF
chmod 644 "$STAGE/DEBIAN/control"

cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi
EOF
chmod 755 "$STAGE/DEBIAN/postinst"

cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi
EOF
chmod 755 "$STAGE/DEBIAN/postrm"

echo "Building $OUT…"
mkdir -p dist
rm -f "$OUT"
dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$OUT" >/dev/null

if command -v lintian >/dev/null 2>&1; then
    echo
    echo "lintian:"
    # The archive-only tag does not apply to a package that is not uploaded.
    lintian --suppress-tags initial-upload-closes-no-bugs "$OUT" || true
fi

echo
echo "Built $OUT"
ls -lh "$OUT"
