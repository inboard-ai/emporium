#!/bin/bash
set -e  # Exit on error

# Build the AlphaVantage extension using cargo-component
echo "Building AlphaVantage extension in release mode..."
cargo component build --release

# Create extension directory if it doesn't exist
EXTENSION_DIR="../../build/xt-alphav"
mkdir -p "$EXTENSION_DIR"

# Copy the WASM component and manifest to the extension directory
echo "Installing extension..."
cp target/wasm32-wasip1/release/xt_alphav.wasm "$EXTENSION_DIR/extension.wasm"
cp manifest.toml "$EXTENSION_DIR/manifest.toml"

echo "Build complete! Extension available at: $EXTENSION_DIR/extension.wasm"
