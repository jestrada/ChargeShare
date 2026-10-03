#!/usr/bin/env bash
# Capture only this repository's offline Rust fixture output, never environment
# dumps, credentials, vehicle input or arbitrary runtime files.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
output=target/spec1-test-results
mkdir -p "$output"
set +e
cargo test --workspace --locked 2>&1 | tee "$output/workspace-tests.txt"
status=${PIPESTATUS[0]}
set -e
# A missing/ignored/filtered acceptance suite must fail, even if Cargo exits 0.
if ! awk '
  /Running tests\/offline_spec1.rs/ { suite = "ledger"; minimum = 17 }
  /Running tests\/offline_pricing.rs/ { suite = "pricing"; minimum = 15 }
  suite != "" && /^test result:/ {
    found[suite] = ($4 + $6 >= minimum && $8 == 0 && $12 == 0)
    suite = ""
  }
  END { exit !(found["ledger"] && found["pricing"]) }
' "$output/workspace-tests.txt" \
   || grep -Eq '; [1-9][0-9]* (ignored|filtered out)' "$output/workspace-tests.txt"; then
  echo 'Offline acceptance suite missing, ignored or filtered' >&2
  status=1
fi
{
  echo '# Offline synthetic ledger and pricing tests'
  echo
  if [[ "$status" -eq 0 ]]; then echo 'Result: PASS'; else echo 'Result: FAIL'; fi
  echo
  grep -E '^(running [0-9]+ tests|test result: |test [a-zA-Z0-9_]+ \.\.\. (ok|FAILED)|error: test failed)' "$output/workspace-tests.txt" || true
} > "$output/summary.md"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  cat "$output/summary.md" >> "$GITHUB_STEP_SUMMARY"
fi
exit "$status"
