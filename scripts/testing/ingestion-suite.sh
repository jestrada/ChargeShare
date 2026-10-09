#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
output=target/ingestion-test-results
mkdir -p "$output"
set +e
cargo test --workspace --locked 2>&1 | tee "$output/workspace-tests.txt"
status=${PIPESTATUS[0]}
set -e
if ! awk '
  /Running tests\/(normalization|persistence|recovery|kafka_driver|cli)\.rs/ {
    suite = $0
    sub(/^.*Running tests\//, "", suite)
    sub(/\.rs.*$/, "", suite)
  }
  suite != "" && /^test result:/ {
    count[suite] = $4
    valid[suite] = ($6 == 0 && $8 == 0 && $12 == 0)
    suite = ""
  }
  END {
    exit !(valid["normalization"] && count["normalization"] >= 20 &&
       valid["persistence"] && count["persistence"] >= 9 &&
       valid["recovery"] && count["recovery"] >= 8 &&
       valid["kafka_driver"] && count["kafka_driver"] >= 10 &&
       valid["cli"] && count["cli"] >= 10)
  }
' "$output/workspace-tests.txt" \
   || grep -Eq '; [1-9][0-9]* (ignored|filtered out)' "$output/workspace-tests.txt"; then
  printf '%s\n' 'Focused ingestion acceptance suite missing, reduced, ignored or filtered' >&2
  status=1
fi
{
  printf '# Focused synthetic ingestion checks\n\n'
  if [[ "$status" -eq 0 ]]; then printf 'Result: PASS\n'; else printf 'Result: FAIL\n'; fi
  printf '\nBoundary: candidate normalization, local SQLite recovery and mocked Kafka.\n'
  printf 'Actual official receiver/Kafka integration acceptance remains unverified.\n\n'
  grep -E '^(running [0-9]+ tests|test result: |test [a-zA-Z0-9_]+ \.\.\. (ok|FAILED)|error: test failed)' "$output/workspace-tests.txt" || true
} > "$output/summary.md"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  cat "$output/summary.md" >> "$GITHUB_STEP_SUMMARY"
fi
exit "$status"
