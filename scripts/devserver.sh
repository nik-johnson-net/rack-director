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

echo "Starting backend server"
cargo build --bin rack-director
cargo run --bin rack-director &

echo "Starting frontend server"
cd rack-director-ui
npx vite build --watch &

wait -n
