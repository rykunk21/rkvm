#!/usr/bin/env bash
set -euo pipefail

# Config
REPO_DIR=$(pwd)               # assuming the script is run from repo root
BIN_DIR=/usr/bin
SYSTEMD_DIR=/usr/lib/systemd/system
CONFIG_DIR=/etc/rkvm

echo "=== RKVM Installer ==="

# Ask for role
while true; do
    read -rp "Is this machine a server or a client? [server/client]: " ROLE
    case "$ROLE" in
        server|Server) ROLE="server"; break ;;
        client|Client) ROLE="client"; break ;;
        *) echo "Please enter 'server' or 'client'." ;;
    esac
done

# Ask user for password
read -rsp "Enter password for RKVM connection: " RKVM_PASSWORD
echo
read -rsp "Confirm password: " RKVM_PASSWORD_CONFIRM
echo

if [ "$RKVM_PASSWORD" != "$RKVM_PASSWORD_CONFIRM" ]; then
    echo "Passwords do not match. Exiting."
    exit 1
fi

# Build binaries
echo "Building rkvm binaries..."
cargo build --release

# Install binaries
echo "Installing binaries to $BIN_DIR..."
sudo cp -f target/release/rkvm-server "$BIN_DIR/rkvm-server"
sudo cp -f target/release/rkvm-client "$BIN_DIR/rkvm-client"
sudo cp -f target/release/rkvm-certificate-gen "$BIN_DIR/rkvm-certificate-gen" 2>/dev/null || true

# Install systemd unit for chosen role
echo "Installing systemd service file for $ROLE..."
if [ "$ROLE" = "server" ]; then
    sudo cp -f systemd/rkvm-server.service "$SYSTEMD_DIR/rkvm-server.service"
else
    sudo cp -f systemd/rkvm-client.service "$SYSTEMD_DIR/rkvm-client.service"
fi

# Setup config directory
echo "Setting up configuration in $CONFIG_DIR..."
sudo mkdir -p "$CONFIG_DIR"

# Server config
if [ "$ROLE" = "server" ]; then
    if [ ! -f "$CONFIG_DIR/server.toml" ]; then
        sudo cp -f examples/server.toml "$CONFIG_DIR/server.toml"
        sudo sed -i "s/^password = .*/password = \"$RKVM_PASSWORD\"/" "$CONFIG_DIR/server.toml"
        echo "Server config written to $CONFIG_DIR/server.toml"
    else
        echo "Server config already exists, skipping."
    fi
fi

# Client config
if [ "$ROLE" = "client" ]; then
    if [ ! -f "$CONFIG_DIR/client.toml" ]; then
        sudo cp -f examples/client.toml "$CONFIG_DIR/client.toml"
        sudo sed -i "s/^password = .*/password = \"$RKVM_PASSWORD\"/" "$CONFIG_DIR/client.toml"
        echo "Client config written to $CONFIG_DIR/client.toml"
    else
        echo "Client config already exists, skipping."
    fi
fi

# Reload systemd and enable/start service
echo "Reloading systemd..."
sudo systemctl daemon-reload

echo "Enabling and starting RKVM service..."
if [ "$ROLE" = "server" ]; then
    sudo systemctl enable --now rkvm-server.service
else
    sudo systemctl enable --now rkvm-client.service
fi

echo "=== RKVM $ROLE installation complete! ==="

