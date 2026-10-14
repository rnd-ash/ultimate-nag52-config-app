#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"

if [[ $(uname -m) != x86_64 ]]; then
    echo "Linux AppImage builds require an x86_64 runner." >&2
    exit 1
fi

appimage_dir=target/appimage
appdir="$appimage_dir/AppDir"
output="$appimage_dir/config_app.AppImage"
mkdir -p "$appimage_dir"
rm -rf "$appdir"
rm -f "$output"

cargo build --release --locked --package config_app --bin config_app

tools_dir=$(mktemp -d)
trap 'rm -rf "$tools_dir"' EXIT
appimagetool="$tools_dir/appimagetool-x86_64.AppImage"
runtime="$tools_dir/runtime-x86_64"

curl --fail --location --retry 3 \
    https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage \
    --output "$appimagetool"
curl --fail --location --retry 3 \
    https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 \
    --output "$runtime"
printf '%s  %s\n' ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 "$appimagetool" | sha256sum --check
printf '%s  %s\n' 2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d "$runtime" | sha256sum --check
chmod +x "$appimagetool"

mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/pixmaps"
cp target/release/config_app "$appdir/usr/bin/config_app"
cp config_app/icon.png "$appdir/usr/share/pixmaps/config_app.png"

cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
set -eu
: "${APPDIR:?AppImage runtime did not set APPDIR}"
export PATH="$APPDIR/usr/bin:$PATH"
export XDG_DATA_DIRS="${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export XDG_DATA_DIRS="$APPDIR/usr/share:$XDG_DATA_DIRS"
exec "$APPDIR/usr/bin/config_app" "$@"
APPRUN
chmod +x "$appdir/AppRun"

version=$(awk -F '"' '/^version = / { print $2; exit }' config_app/Cargo.toml)
test -n "$version"
cat > "$appdir/config_app.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Ultimate NAG52 config suite
Comment=Configuration utility for the NAG52 transmission controller
Exec=config_app
Icon=config_app
Terminal=false
Categories=Utility;
StartupNotify=true
DESKTOP
cp "$appdir/config_app.desktop" "$appdir/usr/share/applications/config_app.desktop"
ln -s usr/share/pixmaps/config_app.png "$appdir/config_app.png"
ln -s config_app.png "$appdir/.DirIcon"

VERSION="$version" ARCH=x86_64 "$appimagetool" --appimage-extract-and-run \
    --runtime-file "$runtime" "$appdir" "$output"
test -s "$output"
chmod +x "$output"
echo "Created $output"
