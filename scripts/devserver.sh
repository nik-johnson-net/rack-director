#!/bin/bash

set -e

function on_exit() {
  if kill -0 %1 >/dev/null 2>&1; then
    kill %1
  fi

  if kill -0 %2 >/dev/null 2>&1; then
    kill %2
  fi
}

function fail() {
  echo "$@" >&2
  exit 1
}

function check_jobs() {
  echo "Checking jobs"
  kill -0 %1 || fail "%1 failed"
  kill -0 %2 || fail "%2 failed"
}

trap on_exit EXIT

cd $(git rev-parse --show-toplevel)

# Create fake files for testing
echo "ipxe fake file" > ".local-storage/tftp/snponly.efi"
echo "undionly.kpxe fake file" > ".local-storage/tftp/undionly.kpxe"
echo "vmlinuz" > ".local-storage/agent-image/vmlinuz"
echo "initramfs" > ".local-storage/agent-image/initramfs.img"

echo "Starting backend server"
cargo build --bin rack-director
LOG=debug cargo run --bin rack-director -- --db-path . --storage-path ./.local-storage/data --tftp-path ./.local-storage/tftp --agent-images-path ./.local-storage/agent-image --dhcp-address 127.0.0.1:1067 --tftp-address 127.0.0.1:1069 --tftp-public-address 127.0.0.1 --http-public-url "http://127.0.0.1" &

echo "Starting frontend server"
cd rack-director-ui
npx vite build --watch &

wait -n
