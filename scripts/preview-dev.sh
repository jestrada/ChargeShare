#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ -x "$repo_root/.tools/cargo/bin/cargo" ]]; then
  export CARGO_HOME="$repo_root/.tools/cargo"
  export RUSTUP_HOME="$repo_root/.tools/rustup"
  export PATH="$CARGO_HOME/bin:$PATH"
fi

if [[ ! -f apps/web/node_modules/vite/bin/vite.js ]]; then
  echo "Install the frontend first: npm --prefix apps/web ci" >&2
  exit 1
fi

cargo build --locked -p chargeshare-preview
"$repo_root/target/debug/chargeshare-preview" &
api_pid=$!
web_pid=""

cleanup() {
  if [[ -n "$web_pid" ]]; then kill "$web_pid" 2>/dev/null || true; fi
  kill "$api_pid" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

sleep 0.5
if ! kill -0 "$api_pid" 2>/dev/null; then wait "$api_pid"; exit 1; fi
cd "$repo_root/apps/web"
node node_modules/vite/bin/vite.js &
web_pid=$!
wait "$web_pid"
