# Tasks

## 1. Canonical baseline

- [x] 1.1 Synchronize and archive the two merged changes. Verify archived artifacts are unchanged and canonical requirements match their completed deltas.

## 2. Requirement coverage

- [x] 2.1 Dated rate table: select dated exact windows without fallback or ambiguity. Verify preview date/precision/replay tests and core `absent_rates_and_gaps_never_fall_back_to_a_nearby_rate`.
- [x] 2.2 Bounded sample totals and rate outlook: derive labels, retain exact arithmetic and expose version provenance/four-decimal rates. Verify preview totals/labels/outlook tests and expanded-session browser checks.
- [x] 2.3 Accessible menus and local preferences: default fresh browsers to Joseph, honor saved defaults and keep sample controls in Settings. Verify fresh/saved/reloaded browser views, keyboard selectors and scenario reset.
- [x] 2.4 Sample reimbursement: show Joseph's eligible energy-only month amount, exclusions, holds and winter uncertainty. Verify preview reimbursement tests plus filter independence, exclusions and all-held browser checks.
- [x] 2.5 Sample vehicle context: keep mobile cards compact with explicit sample freshness/added-energy lines. Verify desktop/mobile browser text and geometry.
- [x] 2.6 Session time-of-use context: expose measured splits, unresolved periods and absent coverage inline. Verify `tou_splits_use_measured_lines_and_do_not_invent_missing_allocations` and expanded/held browser checks.

## 3. Integration verification

- [x] 3.1 Run strict OpenSpec validation, the full local checks and publication safeguards; inspect desktop/mobile evidence and the exact staged diff. Record requirement-to-check evidence in `docs/testing.md`.
