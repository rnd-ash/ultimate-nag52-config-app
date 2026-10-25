#!/usr/bin/env bash
set -euo pipefail

dmg=${1:?Usage: MACOSX_DEPLOYMENT_TARGET=11.0 bash verify-macos-dmg.sh path/to/config_app.dmg}
minimum_macos=${MACOSX_DEPLOYMENT_TARGET:?Set the expected minimum macOS version}

hdiutil verify "$dmg"
mount_point=$(mktemp -d)
launch_log=$(mktemp)
app_pid=""
cleanup() {
    if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
        kill "$app_pid" 2>/dev/null || true
        for _ in {1..10}; do
            process_state=$(ps -o stat= -p "$app_pid" 2>/dev/null || true)
            [[ -z "$process_state" || "$process_state" == *Z* ]] && break
            sleep 0.5
        done
        process_state=$(ps -o stat= -p "$app_pid" 2>/dev/null || true)
        if [[ -n "$process_state" && "$process_state" != *Z* ]]; then
            kill -KILL "$app_pid" 2>/dev/null || true
        fi
    fi
    hdiutil detach "$mount_point" >/dev/null 2>&1 || true
    rmdir "$mount_point" 2>/dev/null || true
    rm -f "$launch_log"
}
trap cleanup EXIT
hdiutil attach "$dmg" -readonly -nobrowse -mountpoint "$mount_point"

app="$mount_point/Ultimate-NAG52-Config-App.app"
plist="$app/Contents/Info.plist"
binary="$app/Contents/MacOS/config_app"

test -x "$binary"
plutil -lint "$plist"
test "$(plutil -extract CFBundleExecutable raw -o - "$plist")" = config_app
test "$(plutil -extract LSMinimumSystemVersion raw -o - "$plist")" = "$minimum_macos"

architectures=$(lipo -archs "$binary")
echo "Bundled architectures: $architectures"
test "$architectures" = arm64

binary_minimum=$(xcrun vtool -show-build "$binary" | awk '$1 == "minos" { print $2 }')
echo "Binary minimum macOS: $binary_minimum"
[[ "$binary_minimum" = "$minimum_macos" || "$binary_minimum" = "$minimum_macos.0" ]]

otool -L "$binary" | awk '
    NR == 1 { next }
    $1 !~ /^\/(usr\/lib\/|System\/Library\/)/ {
        print "Non-system library dependency: " $1
        invalid = 1
    }
    END { exit (invalid || NR < 2) }
'

# Check desktop automation before attributing a missing window to the app.
if ! osascript -e 'tell application "System Events" to tell process "Finder" to get name of every window' >>"$launch_log" 2>&1; then
    cat "$launch_log" >&2
    echo "::warning::macOS GUI startup was NOT checked: desktop automation unavailable."
    if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
        printf '\n### macOS GUI startup: NOT CHECKED\nDesktop automation unavailable. DMG validation passed; publication is allowed.\n' >>"$GITHUB_STEP_SUMMARY"
    fi
    exit 0
fi

open -n "$app" >"$launch_log" 2>&1
window_title=""
for _ in {1..60}; do
    current_app_pid=$(pgrep -x config_app | head -n 1 || true)
    if [[ -n "$current_app_pid" ]]; then
        app_pid=$current_app_pid
        window_title=$(osascript \
            -e 'tell application "System Events" to tell process "config_app" to get name of every window' \
            2>>"$launch_log" || true)
        if [[ "$window_title" == *"Ultimate NAG52 config suite"* ]]; then
            break
        fi
    elif [[ -n "$app_pid" ]]; then
        cat "$launch_log" >&2
        echo "macOS app exited before opening its main window." >&2
        exit 1
    fi
    sleep 0.5
done

if [[ -z "$app_pid" || "$window_title" != *"Ultimate NAG52 config suite"* ]]; then
    cat "$launch_log" >&2
    echo "macOS app did not open its main window within 30 seconds." >&2
    exit 1
fi

sleep 3
process_state=$(ps -o stat= -p "$app_pid" 2>/dev/null || true)
if [[ -z "$process_state" || "$process_state" == *Z* ]]; then
    cat "$launch_log" >&2
    echo "macOS app exited shortly after opening its main window." >&2
    exit 1
fi

echo "Verified macOS DMG and GUI startup: $dmg"
