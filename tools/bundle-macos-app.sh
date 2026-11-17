#!/bin/sh
set -eu

if [ "$#" -ne 4 ] || [ "$(uname -s)" != Darwin ]; then
    printf 'Usage on macOS: %s <release executable> <output.app> <bundle ID> <version>\n' "$0" >&2
    exit 2
fi

binary=$1
app=$2
identifier=$3
version=$4
case "$app" in
    *.app) ;;
    *) printf 'The output must end in .app\n' >&2; exit 2 ;;
esac
if [ ! -f "$binary" ] || [ ! -x "$binary" ]; then
    printf 'Not a runnable executable: %s\n' "$binary" >&2
    exit 2
fi

executable=$(basename "$binary")
name=$(basename "$app" .app)
mkdir -p "$(dirname "$app")"
mkdir "$app"
mkdir -p "$app/Contents/MacOS"
install -m 755 "$binary" "$app/Contents/MacOS/$executable"
plist="$app/Contents/Info.plist"
plutil -create xml1 "$plist"
plutil -insert CFBundleIdentifier -string "$identifier" "$plist"
plutil -insert CFBundleName -string "$name" "$plist"
plutil -insert CFBundleDisplayName -string "$name" "$plist"
plutil -insert CFBundleExecutable -string "$executable" "$plist"
plutil -insert CFBundlePackageType -string APPL "$plist"
plutil -insert CFBundleShortVersionString -string "$version" "$plist"
plutil -insert CFBundleVersion -string 1 "$plist"
plutil -insert NSHighResolutionCapable -bool YES "$plist"
plutil -lint "$plist"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
