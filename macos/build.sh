#!/bin/sh
# Builds macos/build/ezkey.app for the current Mac's architecture.
set -eu
cd "$(dirname "$0")/.."

cargo build --release -p ezkey-core

APP=macos/build/ezkey.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp macos/Info.plist "$APP/Contents/Info.plist"
swiftc -O \
    -import-objc-header core/include/ezkey.h \
    macos/main.swift \
    target/release/libezkey_core.a \
    -o "$APP/Contents/MacOS/ezkey"
# Ad-hoc signature; macOS asks for Accessibility permission again after each rebuild.
codesign --force --sign - "$APP"
echo "Built $APP"
