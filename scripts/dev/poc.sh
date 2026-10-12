#!/usr/bin/env bash
set -euo pipefail
umask 077
cd "$(dirname "$0")/../.."
mkdir -p .cache/tmp .cache/docker .cache/tilt
chmod 700 .cache/tmp .cache/docker .cache/tilt
export TMPDIR="$PWD/.cache/tmp"
export DOCKER_CONFIG="${DOCKER_CONFIG:-$PWD/.cache/docker}"
export WINDMILL_DIR="${WINDMILL_DIR:-$PWD/.cache/tilt}"
exec timeout --signal=TERM --kill-after=30s 20m python3 scripts/dev/poc.py "$@"
