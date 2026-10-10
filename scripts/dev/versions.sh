#!/usr/bin/env bash
set -euo pipefail

nix --version
tilt version
tilt --help > /dev/null
tilt -h > /dev/null
tilt --version > /dev/null
docker --version
docker compose version
docker-compose version
rustc --version
cargo --version
cargo clippy --version
cargo fmt --version
node --version
npm --version
go version
bash --version | sed -n '1p'
openssl version
pkg-config --version
jq --version
curl --version | sed -n '1p'
python3 --version
