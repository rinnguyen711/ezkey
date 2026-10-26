#!/bin/sh
# Builds macos/build/ezkey.app for the current Mac's architecture.
# UNIVERSAL=1 builds an arm64 + x86_64 app instead; it needs
# `rustup target add aarch64-apple-darwin x86_64-apple-darwin`.
set -eu
cd "$(dirname "$0")/.."

APP=macos/build/ezkey.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp macos/Info.plist "$APP/Contents/Info.plist"

if [ "${UNIVERSAL:-}" = 1 ]; then
    for arch in arm64 x86_64; do
        case $arch in
            arm64) rust_target=aarch64-apple-darwin ;;
            x86_64) rust_target=x86_64-apple-darwin ;;
        esac
        cargo build --release -p ezkey-core --target "$rust_target"
        swiftc -O \
            -target "$arch-apple-macos11" \
            -import-objc-header core/include/ezkey.h \
            macos/main.swift \
            "target/$rust_target/release/libezkey_core.a" \
            -o "macos/build/ezkey-$arch"
    done
    lipo -create macos/build/ezkey-arm64 macos/build/ezkey-x86_64 \
        -output "$APP/Contents/MacOS/ezkey"
    rm macos/build/ezkey-arm64 macos/build/ezkey-x86_64
else
    cargo build --release -p ezkey-core
    swiftc -O \
        -import-objc-header core/include/ezkey.h \
        macos/main.swift \
        target/release/libezkey_core.a \
        -o "$APP/Contents/MacOS/ezkey"
fi
# Ad-hoc signature by default; macOS then asks for Accessibility permission
# again after each rebuild. A real identity keeps it, e.g.
# CODESIGN_IDENTITY="Apple Development" sh macos/build.sh
codesign --force --sign "${CODESIGN_IDENTITY:--}" "$APP"
echo "Built $APP"
