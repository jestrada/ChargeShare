#!/usr/bin/env bash
set -euo pipefail
umask 077
cd "$(dirname "$0")/../.."
export CHARGESHARE_PROJECT_NAME="$(python3 scripts/dev/runtime.py project-name)"
export CHARGESHARE_RUNTIME_DIR="$(python3 scripts/dev/runtime.py runtime-dir)"
export CHARGESHARE_BROKER_VOLUME="${CHARGESHARE_PROJECT_NAME}_kafka-data"
export CHARGESHARE_UID="$(id -u)"
export CHARGESHARE_GID="$(id -g)"
compose_environment=$(python3 scripts/dev/runtime.py compose-environment)
eval "$compose_environment"
export CHARGESHARE_SOURCE_EPOCH="$(python3 scripts/dev/runtime.py source-epoch)"
exec target/debug/chargeshare-harness "$@"
