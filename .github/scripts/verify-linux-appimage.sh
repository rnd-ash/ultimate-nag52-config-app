#!/usr/bin/env bash
set -euo pipefail

image=$(realpath "${1:?Usage: verify-linux-appimage.sh path/to/config_app.AppImage}")
test -s "$image"
test -x "$image"

temp_dir=$(mktemp -d)
trap 'rm -rf "$temp_dir"' EXIT
(
    cd "$temp_dir"
    "$image" --appimage-extract >/dev/null
)
appdir="$temp_dir/squashfs-root"
binary="$appdir/usr/bin/config_app"
test -x "$appdir/AppRun"
test -f "$appdir/config_app.desktop"
test -f "$appdir/usr/share/applications/config_app.desktop"
test -f "$appdir/usr/share/pixmaps/config_app.png"
desktop-file-validate "$appdir/config_app.desktop"

dependencies=$(ldd "$binary")
if grep -Fq 'not found' <<<"$dependencies"; then
    printf '%s\n' "$dependencies" >&2
    echo "The AppImage contains unresolved shared-library dependencies." >&2
    exit 1
fi

max_glibc=2.35
required_glibc=$(readelf --version-info "$binary" \
    | grep -Eo 'GLIBC_[0-9]+(\.[0-9]+)+' \
    | sed 's/GLIBC_//' \
    | sort -Vu \
    | tail -n 1)
if [[ -z $required_glibc || $(printf '%s\n%s\n' "$required_glibc" "$max_glibc" | sort -V | tail -n 1) != "$max_glibc" ]]; then
    echo "AppImage requires GLIBC $required_glibc; maximum supported build baseline is $max_glibc." >&2
    exit 1
fi
echo "Verified AppImage GLIBC requirement: $required_glibc (baseline $max_glibc)"

log_file="$temp_dir/startup.log"
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET \
    LIBGL_ALWAYS_SOFTWARE=1 \
    RUST_LOG=wgpu_hal::gles::egl=debug,wgpu_core::instance=debug \
    xvfb-run -a bash -s -- "$image" "$log_file" <<'SMOKE'
set -euo pipefail
image=$1
log_file=$2
"$image" --appimage-extract-and-run >"$log_file" 2>&1 &
app_pid=$!
cleanup() {
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
}
trap cleanup EXIT

if ! timeout 30s xdotool search --sync --onlyvisible --name '^Ultimate NAG52 config suite$' >/dev/null; then
    cat "$log_file" >&2
    echo "Linux app did not open its main window within 30 seconds." >&2
    exit 1
fi
sleep 3
process_state=$(ps -o stat= -p "$app_pid" 2>/dev/null || true)
if [[ -z $process_state || $process_state == *Z* ]]; then
    cat "$log_file" >&2
    echo "Linux app exited shortly after opening its main window." >&2
    exit 1
fi
SMOKE

echo "Verified Linux AppImage and GUI startup: $image"
