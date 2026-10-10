#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
export CHARGESHARE_PROJECT_NAME="$(python3 scripts/dev/runtime.py project-name)"
export CHARGESHARE_RUNTIME_DIR="$(python3 scripts/dev/runtime.py runtime-dir)"
export CHARGESHARE_BROKER_VOLUME="${CHARGESHARE_PROJECT_NAME}_kafka-data"
export CHARGESHARE_UID="$(id -u)"
export CHARGESHARE_GID="$(id -g)"
tilt down
python3 scripts/dev/runtime.py reset --confirm-synthetic-reset
