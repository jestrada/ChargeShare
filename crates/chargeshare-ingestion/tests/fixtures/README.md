# Fictional normalization fixtures

`candidate-v.json` contains only the six decoded `complete.expected` records from
the stage-1 synthetic fixture at receiver revision
`bd076fe1494841707528449560c4a19d0d426da4`. Its metadata explicitly records that
actual receiver/Kafka transport remains unverified. These fixtures exercise the
inspected candidate V shape, not an integration acceptance boundary. Binary frames,
acknowledgments, certificates and runtime context are excluded.

`manifest-v1.json` is a handwritten, synthetic-only ChargeShare annotation map.
Its fictional device/vehicle/owner aliases, connections, positions, event kinds
and AC labels are annotations. Source times and counter observations come from
candidate records. The candidate counters yield 0.5 and 0.25 kWh; tests that use
10 and 4 kWh explicitly create separate handwritten focused-consumer records.
No annotation contains an expected counter or energy result.

All adversarial input strings and additional records in `normalization.rs` are
handwritten fictional focused-normalizer test inputs. None is captured vehicle,
location, credential or account data. Omitting an observed End record leaves its
annotation unused; charging-complete state, transport resends and silence supply
no extra event.

## Actual decoded golden handoff

Issue #27's sandbox cannot access Docker, so no actual decoded receiver golden is
committed by this change. Candidate provenance remains unchanged. On supported
Linux, run `nix develop --no-update-lock-file -c bash scripts/dev/acceptance.sh all`.
After successful actual receiver/Kafka multiset and normalizer checks, the private
capture survives teardown at `.cache/ingestion-acceptance/decoded-capture.json`.
Review its fictional fields against the pinned receiver contract, add the golden
here and exercise its normalization and rejection cases before claiming that
acceptance item. No raw capture is included in hosted artifacts.
