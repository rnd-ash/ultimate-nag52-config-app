#!/usr/bin/env bash
set -euo pipefail

dmg=${1:?Usage: MACOSX_DEPLOYMENT_TARGET=11.0 bash verify-macos-dmg.sh path/to/config_app.dmg}
minimum_macos=${MACOSX_DEPLOYMENT_TARGET:?Set the expected minimum macOS version}

hdiutil verify "$dmg"
mount_point=$(mktemp -d)
trap 'rmdir "$mount_point"' EXIT
hdiutil attach "$dmg" -readonly -nobrowse -mountpoint "$mount_point"
trap 'hdiutil detach "$mount_point"; rmdir "$mount_point"' EXIT

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
