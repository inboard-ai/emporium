#!/bin/bash
set -e  # Exit on error

# Build the Polygon extension in release mode
echo "Building Polygon extension in release mode..."
cargo build --target wasm32-unknown-unknown --release

# Convert the WASM module to a component
echo "Converting WASM module to component..."
wasm-tools component new \
    target/wasm32-unknown-unknown/release/emporium_polygon.wasm \
    -o target/wasm32-unknown-unknown/release/emporium_polygon_component.wasm

# Create extension directory if it doesn't exist
EXTENSION_DIR="../../build/emporium_polygon"
mkdir -p "$EXTENSION_DIR"

# Copy the WASM component and manifest to the extension directory
echo "Installing extension..."
cp target/wasm32-unknown-unknown/release/emporium_polygon_component.wasm "$EXTENSION_DIR/emporium_polygon.wasm"
cp manifest.toml "$EXTENSION_DIR/manifest.toml"

echo "Build complete! Extension available at: $EXTENSION_DIR/emporium_polygon.wasm"
