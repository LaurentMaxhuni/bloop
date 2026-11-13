#!/usr/bin/env bash
# Builds dist/bloop-<version>-x86_64.AppImage and its .sha256 (the format the updater reads:
# "<hex>  <file name>"). Run from anywhere on x86_64 Linux:
#   installer/linux/build-appimage.sh [version]
# The version defaults to Cargo.toml's. Set SKIP_CARGO=1 to reuse target/release/bloop.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

version="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)}"
[ -n "$version" ] || { echo "No version in Cargo.toml." >&2; exit 1; }

# Pinned appimagetool, checked against its published SHA-256 before it runs.
tool_url="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
tool_sha="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"

if [ "${SKIP_CARGO:-}" != 1 ]; then
    cargo build --release --locked
fi

work="$root/target/appimage"
rm -rf "$work"
mkdir -p "$work/AppDir/usr/bin" "$root/dist"

tool="$work/appimagetool"
curl -fsSL --retry 3 -o "$tool" "$tool_url"
echo "$tool_sha  $tool" | sha256sum -c --quiet -
chmod +x "$tool"

app="$work/AppDir"
install -m 755 target/release/bloop "$app/usr/bin/bloop"
install -m 644 installer/linux/bloop.desktop "$app/bloop.desktop"
install -m 644 installer/linux/bloop.png "$app/bloop.png"
ln -s usr/bin/bloop "$app/AppRun"

name="bloop-$version-x86_64.AppImage"
# CI runners have no FUSE: let appimagetool run itself unpacked.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream "$app" "dist/$name"

(cd dist && sha256sum "$name" > "$name.sha256")
echo "Built dist/$name ($(du -h "dist/$name" | cut -f1))"
