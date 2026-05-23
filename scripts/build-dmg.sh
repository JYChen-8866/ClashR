#!/bin/bash
set -e

# Build DMG for ClashR on macOS
# Usage: ./scripts/build-dmg.sh

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="ClashR"
VERSION="0.1.0"
BUNDLE_ID="com.clashr.app"

BUILD_DIR="$PROJECT_ROOT/target/release"
DMG_DIR="$PROJECT_ROOT/target/dmg"
APP_DIR="$DMG_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

echo "==> Building release binary..."
cargo build --release

echo "==> Creating .app bundle structure..."
rm -rf "$DMG_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

echo "==> Copying binaries..."
cp "$BUILD_DIR/clashr" "$MACOS_DIR/$APP_NAME"
cp "$PROJECT_ROOT/bin/mihomo" "$MACOS_DIR/mihomo"
cp "$PROJECT_ROOT/bin/clashr-service" "$MACOS_DIR/clashr-service"
chmod +x "$MACOS_DIR"/*

echo "==> Copying resources..."
cp -r "$PROJECT_ROOT/icons" "$RESOURCES_DIR/"

echo "==> Creating app icon..."
# Convert SVG to high-quality PNG at 1024x1024
rsvg-convert -w 1024 -h 1024 "$PROJECT_ROOT/icons/app/app-icon.svg" -o "$RESOURCES_DIR/AppIcon.png"

echo "==> Writing Info.plist..."
cat > "$CONTENTS_DIR/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF

echo "==> Creating DMG..."
DMG_NAME="$APP_NAME-$VERSION-arm64.dmg"
DMG_PATH="$PROJECT_ROOT/target/$DMG_NAME"
rm -f "$DMG_PATH"

# Add Applications symlink so users can drag-install directly from the DMG
ln -sf /Applications "$DMG_DIR/Applications"

# Create compressed DMG
hdiutil create -volname "$APP_NAME" -srcfolder "$DMG_DIR" -ov -format UDZO "$DMG_PATH"

# Clean up symlink
rm -f "$DMG_DIR/Applications"

echo "==> Done!"
echo "    App bundle: $APP_DIR"
echo "    DMG: $DMG_PATH"
