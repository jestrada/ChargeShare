#!/usr/bin/env bash
set -euo pipefail
umask 077
cd "$(dirname "$0")/../.."
mkdir -p .cache/ingestion-acceptance/tmp .cache/ingestion-acceptance/docker .cache/ingestion-acceptance/tilt
chmod 700 .cache/ingestion-acceptance .cache/ingestion-acceptance/tmp .cache/ingestion-acceptance/docker .cache/ingestion-acceptance/tilt
export TMPDIR="$PWD/.cache/ingestion-acceptance/tmp"
export DOCKER_CONFIG="${DOCKER_CONFIG:-$PWD/.cache/ingestion-acceptance/docker}"
export WINDMILL_DIR="${WINDMILL_DIR:-$PWD/.cache/ingestion-acceptance/tilt}"
exec timeout --signal=TERM --kill-after=60s 40m python3 scripts/dev/acceptance.py "$@"
