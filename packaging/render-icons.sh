#!/bin/sh
# Regenerate the PNG app icons from selkie.svg.
# Requires rsvg-convert, Inkscape or ImageMagick.
set -eu

cd "$(dirname "$0")"

SIZES="16 32 48 64 128 256 512"
mkdir -p icons

for size in $SIZES; do
    out="icons/selkie-$size.png"
    if command -v rsvg-convert >/dev/null 2>&1; then
        rsvg-convert -w "$size" -h "$size" selkie.svg -o "$out"
    elif command -v inkscape >/dev/null 2>&1; then
        inkscape --export-type=png --export-filename="$out" \
            --export-width="$size" --export-height="$size" \
            selkie.svg >/dev/null 2>&1
    elif command -v magick >/dev/null 2>&1; then
        magick -background none selkie.svg -resize "${size}x${size}" "$out"
    elif command -v convert >/dev/null 2>&1; then
        convert -background none selkie.svg -resize "${size}x${size}" "$out"
    else
        echo "error: need rsvg-convert, Inkscape or ImageMagick to render icons" >&2
        exit 1
    fi
    echo "rendered $out"
done
