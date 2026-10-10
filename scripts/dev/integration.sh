#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
output=target/harness-results
mkdir -p "$output"
summary="$output/summary.md"
printf '# Synthetic receiver-to-Kafka verification\n\nCommit: %s\nRunner: %s / %s\nBoundary: receiver ACK plus decoded Kafka records; no adapter, SQLite or receiver-backed dashboard.\n\n' "$(git rev-parse HEAD)" "$(uname -s)" "$(uname -m)" > "$summary"
export CHARGESHARE_PROJECT_NAME="$(python3 scripts/dev/runtime.py project-name)"
export CHARGESHARE_RUNTIME_DIR="$(python3 scripts/dev/runtime.py runtime-dir)"
export CHARGESHARE_BROKER_VOLUME="${CHARGESHARE_PROJECT_NAME}_kafka-data"
export CHARGESHARE_UID="$(id -u)"
export CHARGESHARE_GID="$(id -g)"
compose=(docker compose --project-name "$CHARGESHARE_PROJECT_NAME" --file dev/compose.yaml)
stage=setup

record() {
  printf '%s\n' "$1" | tee -a "$summary"
}

cleanup() {
  result=$?
  trap - EXIT INT TERM
  set +e
  tilt down > "$output/teardown-local.log" 2>&1
  stop_result=$?
  "${compose[@]}" down --timeout 10 >> "$output/teardown-local.log" 2>&1
  python3 scripts/dev/runtime.py reset --confirm-synthetic-reset >> "$output/teardown-local.log" 2>&1
  reset_result=$?
  if [[ "$stop_result" -ne 0 || "$reset_result" -ne 0 ]]; then
    record 'FAIL: project teardown did not complete; inspect only the local teardown log.'
    if [[ "$result" -eq 0 ]]; then result=1; fi
  fi
  if [[ "$result" -eq 0 ]]; then record 'Result: PASS'; else record "Result: FAIL at $stage"; fi
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then cat "$summary" >> "$GITHUB_STEP_SUMMARY"; fi
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

expect_failure() {
  description=$1
  shift
  if "$@" > "$output/expected-failure-local.log" 2>&1; then
    record "FAIL: $description unexpectedly succeeded"
    return 1
  fi
  record "PASS: $description returned a failure exit status"
}

start_stack() {
  stage=startup
  tilt up --stream > "$output/tilt-local.log" 2>&1 &
  tilt_pid=$!
  api_deadline=$((SECONDS + 90))
  until tilt get uiresource '(Tiltfile)' -o name > /dev/null 2>&1; do
    if ! kill -0 "$tilt_pid" 2>/dev/null; then
      wait "$tilt_pid" || return $?
      return 1
    fi
    if [[ "$SECONDS" -ge "$api_deadline" ]]; then record 'FAIL: Tilt API startup exceeded 90 seconds'; return 1; fi
    sleep 1
  done
  tilt wait --for=condition=Ready 'uiresource/(Tiltfile)' --timeout 90s
  python3 scripts/dev/readiness.py --timeout 900 kafka receiver rust-build receiver-ready | tee -a "$summary"
  python3 scripts/dev/readiness.py --timeout 300 frontend-install fixture-api fixture-dashboard | tee -a "$summary"
  record 'PASS: protocol/status readiness and independent fixture preview'
}

stop_stack() {
  stage=shutdown
  tilt down
  python3 - <<'PY'
from scripts.dev.runtime import check_ports_available
check_ports_available()
PY
  docker volume inspect "$CHARGESHARE_BROKER_VOLUME" --format '{{.Name}}' > /dev/null
  record 'PASS: shutdown released listeners and preserved the named broker volume'
}

python3 scripts/dev/test_runtime.py
python3 scripts/dev/test_readiness.py
python3 scripts/dev/test_cached_receiver.py
record 'PASS: project preflight, privacy, certificate and reset guardrails'
python3 scripts/dev/runtime.py preflight > "$output/preflight-local.json"
record "Host Docker daemon: $(jq -r .docker_version "$output/preflight-local.json")"
for iteration in first second; do
  start_stack
  stage=manual-smoke
  previous_deploy=$(tilt get uiresource telemetry-smoke -o json | jq -r '.status.lastDeployTime // ""')
  tilt trigger telemetry-smoke
  smoke_deadline=$((SECONDS + 90))
  while true; do
    current_deploy=$(tilt get uiresource telemetry-smoke -o json | jq -r '.status.lastDeployTime // ""')
    if [[ -n "$current_deploy" && "$current_deploy" != "$previous_deploy" ]]; then break; fi
    if [[ "$SECONDS" -ge "$smoke_deadline" ]]; then record 'FAIL: explicitly triggered telemetry-smoke did not complete a new successful build within 90 seconds'; exit 1; fi
    sleep 1
  done
  tilt wait --for=condition=Ready uiresource/telemetry-smoke --timeout 10s
  record 'PASS: explicitly triggered telemetry-smoke requires ACK plus Kafka output'
  if [[ "${CHARGESHARE_CI_PROVE_FAILURE:-0}" == 1 ]]; then
    stage=deliberate-missing-output-gate-proof
    bash scripts/dev/harness.sh smoke --inject-missing-output
    record 'FAIL: deliberately missing output unexpectedly passed'
    exit 1
  fi
  stage=fixture-scenarios
  bash scripts/dev/harness.sh all
  cp "$CHARGESHARE_RUNTIME_DIR/normalized.json" "$output/normalized-$iteration.json"
  record "PASS: $iteration clean run preserved complete/missing/duplicate/out-of-order records"
  stage=retention
  bash scripts/dev/harness.sh replay
  bash scripts/dev/harness.sh replay
  record 'PASS: independent verifier processes re-read retained synthetic records'
  if [[ "$iteration" == first ]]; then
    stage=negative-scenarios
    expect_failure 'stale records cannot satisfy a new smoke run' bash scripts/dev/harness.sh smoke --no-send
    expect_failure 'ACK without a complete expected Kafka multiset fails' bash scripts/dev/harness.sh smoke --inject-missing-output
    bash scripts/dev/harness.sh invalid-auth
    expect_failure 'active resources refuse destructive reset' python3 scripts/dev/runtime.py reset --confirm-synthetic-reset
    "${compose[@]}" stop kafka
    expect_failure 'broker outage refuses the run at offset validation before sending' bash scripts/dev/harness.sh smoke
    "${compose[@]}" start kafka
    timeout 120 bash -c 'until bash scripts/dev/harness.sh ready >/dev/null 2>&1; do sleep 2; done'
    record 'PASS: missing output, invalid authentication and broker outage remain bounded failures'
    stop_stack
    start_stack
    bash scripts/dev/harness.sh replay
    record 'PASS: normal stop/start preserves replayable broker records'
  fi
  if [[ "$iteration" == second ]]; then
    stage=shutdown-with-incomplete-trust
    mv "$CHARGESHARE_RUNTIME_DIR/certificates/client.crt" "$CHARGESHARE_RUNTIME_DIR/certificates/client.crt.saved"
  fi
  stop_stack
  if [[ "$iteration" == second ]]; then
    record 'PASS: incomplete test trust did not prevent owned container shutdown'
  fi
  stage=synthetic-reset
  python3 scripts/dev/runtime.py reset --confirm-synthetic-reset
  record "PASS: $iteration explicit stopped-project reset"
done
stage=deterministic-results
cmp "$output/normalized-first.json" "$output/normalized-second.json"
record 'PASS: two full reset/start/scenario paths produced identical semantic results'
record "Elapsed integration seconds: $SECONDS"
