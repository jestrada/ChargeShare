# Tasks

Planning only. Every task remains unchecked until separately approved implementation and its stated evidence exist. No local services or receiver-backed/UI acceptance were run while preparing this change. Stage 1 is verified and archived at published head `33a4568`; stage 2 retains its unfinished tasks. Recording prerequisite evidence here does not complete this proposed implementation.

## 1. Establish the prerequisite gates

- [ ] 1.1 Record stage 1's published exact implementation commit, pinned receiver/schema/trust/topic and terminal Linux receiver→Kafka acceptance; verify the recovered `ec1c892` ACK fixes are preserved and `CHARGESHARE_CI_PROVE_FAILURE=1` failed before a separately restored successful run. Keep its implementation/failure-proof evidence in the archived `2026-10-10-local-nix-tilt-kafka-harness` change.
- [ ] 1.2 Record stage 2's owned broker-epoch lifecycle, actual receiver normalization/SQLite/replay acceptance and retention refusal; verify its remaining integration/CI tasks are completed under `durable-synthetic-ingestion` rather than checking them through this proposal.
- [ ] 1.3 Document the exact support/prerequisite matrix, receiver fixture's 0.5/0.25 kWh versus focused 10/4 tests, private runtime paths and pinned build closure; verify inspected inputs and checks against the archived stage-1 and active stage-2 changes before implementing their dependent path.

## 2. Add the single persisted Rust runtime

- [ ] 2.1 Add one outer Rust runtime mode owning Kafka consumption, SQLite and bounded API commands; retain the standalone finite ingestion CLI and verify competing-writer refusal, request/poll deadlines, shutdown ownership and unchanged inward core dependencies with focused tests.
- [ ] 2.2 Add transactional schema migration for scoped semantic evidence revisions, review revisions and immutable rate selections; verify preserved evidence/progress, interrupted/failed migration rollback, unsupported/newer-schema refusal and private DB/WAL/SHM ownership.
- [ ] 2.3 Define committed scoped snapshot and safe readiness/error DTOs; verify concurrent ingestion returns coherent old-or-new versions and duplicate delivery does not change semantic energy or invalidate an applicable review. Document API/provenance and the single-owner responsibility in the same group.

## 3. Persist conservative review and exact rates

- [ ] 3.1 Add explicit scope/session/evidence/review-version checked classification updates; verify default Unconfirmed/zero eligible, selected SharedCharger persistence after restart, OtherCharger/Unconfirmed reversal, wrong-scope refusal and no clearing of quality/DC/ambiguous holds.
- [ ] 3.2 Make prior classification visibly stale on distinct changed evidence until explicit reconfirmation; verify stale update rejection, retained prior revision and stable applicable review on identical semantic redelivery. Document manual review boundaries without adding free-text or real reviewer identity.
- [ ] 3.3 Persist immutable copies of the existing labeled public sample rates and explicit synthetic UTC window-conversion provenance; verify exact decimal parsing, effective coverage, overlap/time-domain/same-version-content refusal and no neighbor-rate fallback. Document epoch-seconds versus old preview ticks without adding a real utility calendar.
- [ ] 3.4 Reconstruct and price the scoped committed snapshot with the existing core; verify observed/eligible/priced energy separation, line/rate/calculation provenance, missing-rate/boundary holds, exact aggregate rounding and overflow failure. Independently core-check confirmed actual-fixture exact costs 0.1386965510000000/0.0693482755000000 USD and $0.14/$0.07 scoped subtotals.

## 4. Connect scoped API and persisted dashboard

- [ ] 4.1 Add bounded persisted result and deliberate review endpoints with owner/vehicle/session/version checks, safe fixed errors, intended Origin/Host/content-type validation and no unscoped combined endpoint; verify cross-owner reads/reviews and unexpected-origin mutation fail without disclosure/mutation.
- [ ] 4.2 Add the explicit persisted dashboard read path and API-derived amounts, current/stale review state, hold counts and expandable rate/version provenance; verify no frontend money arithmetic, invented battery/state values or fixture fallback. Keep fixture API/scenarios and browser display preferences unchanged and document each mode/URL.
- [ ] 4.3 Add automated frontend/API contract checks for loading, unavailable/incompatible API, empty/valid-zero/all-held states, selected scope and deliberate review updates; verify persisted network data matches its scoped API and reviews/rates are not saved as browser preferences.
- [ ] 4.4 Execute and record manual desktop 1280px/mobile 375px persisted UI smoke: actual network responses, keyboard/tap filters/details/review, stale holds, rate provenance, no overflow and unavailable-API behavior; keep this unchecked until executed and attach only safe synthetic evidence.

## 5. Make every service runnable and stoppable

- [ ] 5.1 Extend Tilt/Compose with a pinned single Rust runtime in the private Kafka network and separate persisted dashboard; verify exact allowlisted image/build/mount/UID configuration with only private application DB state read/write and individual config/epoch files read-only, no application access to receiver keys, no Kafka host listener and only IPv4-loopback API/frontend publication. Preserve existing fixture resource names and document all resource endpoints/logs.
- [ ] 5.2 Implement `nix develop --no-update-lock-file -c bash scripts/dev/poc.sh up` and explicit `check` using the existing lifecycle guards; verify locked dependency build/install, all resources, no implicit acceptance stream, bounded receiver/Kafka/application/UI readiness and setup/port/ownership failure. Execute the commands as documented on supported Linux.
- [ ] 5.3 Implement `restart` and `down`; verify supervised signal handling, stopped container/host processes, released listeners/locks and preserved epoch/broker/DB/reviews/rates with unchanged scoped semantic results after restart. Document stop versus acceptance versus reset.
- [ ] 5.4 Extend narrow reset for the exact embedded DB/companions and existing owned broker/runtime state; verify `reset --confirm-synthetic-reset` requires stopped resources, refuses missing confirmation/ownership/symlink mismatch, preserves unrelated checkouts and regenerates a new epoch on next startup. Execute two reset/start/check cycles and compare semantic expectations.

## 6. Prove the complete receiver-backed results path

- [ ] 6.1 Add finite run-isolated acceptance through actual receiver ACK, newly observed Kafka, curated durable dispositions/progress, scoped API and persisted-dashboard network reads; verify the unreviewed baseline and explicit scoped review/covered-rate results against independent core reference construction. Prove stale records, missing output, ACK-only, direct Kafka injection and fixture API cannot satisfy it.
- [ ] 6.2 Exercise real owning-process terminations at synchronized pre-commit and post-commit boundaries plus an indeterminate commit window; verify recovered atomic evidence/progress, stable session IDs/reviews/rates and no duplicate energy/cost after restart/forced reread. Document what kill timing was actually proven.
- [ ] 6.3 Exercise receiver-backed duplicate/reordered/late input, conflicts, missing boundary/rate and broker interruption; verify deterministic final scoped results, stale-review behavior and preserved holds while the other vehicle is unaffected. Keep focused direct-input checks labeled supplementary.
- [ ] 6.4 Exercise retained-offset gap, changed source epoch and incompatible/corrupt storage; verify runtime/readiness/check failure preserves prior state and rejects stale-current results rather than deleting data or seeking latest. Document supported recovery and local-storage guarantee limits.

## 7. Gate, review and record the final local POC

- [ ] 7.1 Add a dedicated `ubuntu-24.04` x86_64 complete-path CI job reusing pinned Nix/Tilt/Compose and no external credentials/secrets; verify hard setup/observation/API/job deadlines, preserved nonzero pipeline status and owned-resource teardown on setup/assertion/timeout/signal failures.
- [ ] 7.2 Prove a deliberately wrong persisted expected output/energy/cost fails the gate, then restore it and record a separate terminal passing exact-commit hosted run; inspect only allowlisted synthetic summaries for absent raw payloads/logs/DB dumps/certificates/keys and verify cleanup failure remains visible.
- [ ] 7.3 Run existing offline/ingestion suites, Rust formatting/Clippy, frontend build, strict OpenSpec validation, security guard tests/scans and exact diff/path review; verify no ignored/filtered/reduced existing scenarios, no authored code comments, coherent responsibility boundaries and preserved fixture behavior. Update affected architecture, data-flow and dependency diagrams in the same PR, with verified/proposed status and Mermaid render evidence or a disclosed limitation.
- [ ] 7.4 After review, implementation and actual acceptance, sync only this accepted capability into canonical specs and archive only this change on its implementation PR; verify synced requirements, archived task evidence, final remote commit and hosted checks before any separately authorized merge. Leave incomplete prerequisite changes active.

External setup remains outside these tasks: no live Tesla registration/OAuth/key pairing, new external credentials, real data, purchases, Cloudflare provisioning/Terraform apply, public deployment or merge is authorized.
