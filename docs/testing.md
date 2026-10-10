# Testing and verification

Spec 1 has been approved for implementation and now has an offline Rust suite.
The approved local receiver-to-Kafka harness is described in [local development](local-development.md). Its adapter, durable ingestion and receiver-backed dashboard remain later changes.

Offline session pricing adds 15 acceptance tests in
`crates/chargeshare-core/tests/offline_pricing.rs` and five unit tests for money
and tariff primitives. The existing 17 acceptance tests and two energy tests are
unchanged. The [pricing contract](pricing.md) describes the implemented scope.
The preview adds twelve Rust route/fixture/rate-table tests, for 51 workspace tests total.
Its React frontend has a TypeScript/production-build check and local browser
verification described in [local preview](local-preview.md).

## What runs today

[Repository checks](../.github/workflows/security.yml) runs strict OpenSpec
validation, security guard tests/history scans, the frontend build, Rust formatting, clippy and the
complete `cargo test --workspace --locked` on every push and pull request. The
pinned Rust 1.99.0 toolchain and Cargo.lock are used on clean hosted runners.
The wrapper `bash scripts/testing/offline-suite.sh` preserves Cargo's exit status,
fails on absent/ignored/filtered acceptance tests, fewer than 17 ledger scenarios
or fewer than 15 pricing scenarios, and produces a safe synthetic
summary plus test log. The allowlisted artifact has seven-day retention and is
uploaded on success or failure. No `continue-on-error`, credentials or network
calls are used in the domain suite. Dependency/tool downloads are setup only.

## Spec 1 scenario coverage

All scenarios are exercised in `crates/chargeshare-core/tests/offline_spec1.rs`:

- Two interleaved vehicles, independent 10/4 kWh counter boundaries and independent
  confirmation: `interleaved_ten_and_four_are_separate_and_independently_confirmed`
- Unknown/missing identities and ambiguous registration:
  `rejected_identity_and_duplicate_registration_leave_all_state_unchanged`
- Cross-vehicle review and independent reads:
  `scoped_reads_and_cross_vehicle_review_never_mutate_other_vehicle`
- Duplicates/late evidence:
  `shuffled_duplicates_and_late_evidence_preserve_ids_reviews_flags_and_totals`
- Matching cross-vehicle events:
  `identical_events_across_vehicles_are_not_deduplicated_together`
- Conflicting evidence and interior uncertainty barriers:
  `same_position_conflicts_hold_all_affected_connections_independent_of_arrival`
  and `interior_conflicts_break_counter_chain_and_retain_all_safe_evidence_reasons`
- Pause/resume and new connection:
  `pause_resume_stays_in_connection_and_new_connection_never_bridges_counters`
- DC/ambiguous evidence:
  `dc_ambiguous_and_mixed_type_evidence_is_explicitly_excluded`
- Bad/rollback counter and confirmation cannot override evidence:
  `rollback_counts_only_valid_positive_deltas_and_confirmation_cannot_clear_flags`
  and `invalid_negative_precision_and_overflow_counters_retain_safe_flags_no_invented_delta`
- Missing start/end/baseline/terminal and silence:
  `missing_baseline_terminal_and_boundaries_stay_visible`
  and `silence_never_fabricates_end_time_baseline_or_consumption`
- Exact parsing, accumulation overflow and deterministic tie/boundary ordering:
  the remaining arithmetic and boundary tests
- Run without Tesla access: all fixtures construct only fictional domain values;
  there are no network or credential dependencies in Cargo.toml

See the [domain contract](architecture.md#implemented-offline-ledger) for precision,
input rules, ordering/conflicts and physical/security limits, and [SECURITY.md](../SECURITY.md)
for publication safeguards. Verification evidence is retained below.

## Spec 1 verification

Implementation approved and applied on 2026-10-02. Only synthetic offline Rust
behavior is delivered; physical accuracy, user authorization, persistence, actual
Go receiver integration and live-car validation remain outside this milestone.
The historical review/planning context is retained in the
[archived OpenSpec change](../openspec/changes/archive/2026-10-03-offline-multi-vehicle-ledger/proposal.md).
Its seven accepted requirements are synced to the
[main vehicle-ledger spec](../openspec/specs/vehicle-ledger/spec.md).

### Local checks

- Rust 1.99.0: formatting and clippy with warnings denied passed
- `cargo test --workspace --locked`: 17 passed, zero failed/ignored/filtered
- `bash scripts/testing/offline-suite.sh`: same complete command and passing summary
- Checked exact six-decimal arithmetic and session/aggregate overflow
- Every behavioral Spec 1 scenario mapped above
- Independent review identified an interior conflicting sample could bridge an
  uncertain AC interval; fixed by conflict barriers retaining all safe evidence
  reasons, with forward/reverse regression coverage

- Strict OpenSpec validation: 1 change passed, zero failed
- Security guard tests: safe scans passed; 17 sensitive paths and a synthetic token blocked
- Staged and all-history Gitleaks 8.30.1 scans: passed, zero leaks
- Exact staged paths/diff reviewed: only public-safe Rust source, fictional fixture
  values, CI script/workflow and project/spec status documentation

These checks are repeated before each publication. Hosted deliberate-failure and
restored passing-run evidence is recorded below.

### Code design refactor verification

The user-approved 2026-10-02 behavior-preserving refactor applies the
[module ownership decision](architecture.md#core-module-ownership). The public
crate-root API and all 17 offline acceptance scenarios are unchanged; two focused
unit tests cover the crate-private checked delta at zero/equality, maximum energy
and rollback. No feature, dependency, live adapter or infrastructure is added.

- Pinned Rust 1.99.0 formatting and clippy with warnings denied: passed
- Complete workspace suite through `offline-suite.sh`: 19 passed, including all
  17 unchanged acceptance scenarios; zero failed, ignored or filtered
- Strict OpenSpec validation and all security guard fixtures: passed
- A temporary differential replay probe compared baseline and refactored results
  for 1,000 deterministic synthetic sequences containing 120,000 events, plus
  parser outcomes, duplicate ingestion, scoped reads/reviews, conflicts,
  incomplete evidence and overflow: every snapshot matched
- Independent read-only review compared public declarations/signatures, replay,
  delta arithmetic, module dependencies and documentation against the baseline:
  no blockers or actionable findings; the reviewer also repeated the full local
  formatting, clippy, workspace, OpenSpec and guard checks
- Core implementation source contains no code or documentation comments;
  contracts and rationale are retained in these three guides

Staged/history scans and exact diff review are repeated before publication.
Hosted checks must pass on the actual final refactor commit; the historical runs
below do not establish that result.

### Hosted gate evidence

- Initial correct implementation `f4e30001a197dbf4398be259989808ff9c379f74`:
  GitHub Actions push run `37060010078` and PR run `37060017575`
  passed all three jobs from clean hosted runners
- Deliberate wrong-total probe `4fe4a2d9a7815a59a45c2fa6a1f69fdc94bed3c6`
  changed only the expected vehicle-a total from 10 to 11 kWh.
  The normal local test command and CI wrapper both exited 101.
  Hosted GitHub Actions run `37060083478`
  failed its Rust job at `Test workspace`: actual 10 kWh, expected 11 kWh;
  16 passed, 1 failed, 0 ignored/filtered. OpenSpec and security still passed.
  The always-running artifact step succeeded, retaining
  `offline-spec1-test-results` (artifact 11250295410, seven-day retention).
  No errors were masked and no environment/credential/real-data dump was uploaded.
- Correct 10 kWh assertion restored in `349e19e0fccb615db3f7ca8fda13986fc5f8d7ba`:
  GitHub Actions push run `37060298181` and PR run `37060303844`
  passed all three jobs, with all 17 domain tests passing
- Final application/test source `7df1b916774b973ec63ca4c0506b3ed21328db1e`
  also labels each session explicitly as synthetic and physically unvalidated:
  GitHub Actions push run `37060409228` and PR run `37060416344`
  both passed. Subsequent completion edits change documentation/task state only.
  Verify each exact publication head and its terminal hosted checks before
  reporting completion; historical runs do not verify later commits.

Independent read-only review approved the corrected core and CI wrapper; eight
isolated guard smoke cases verified propagation of exit 101 and rejection of
missing, zero, reduced, ignored or filtered acceptance suites. There are no
outstanding review blockers. Review does not replace the hosted final-SHA check.

## Offline pricing and local preview verification

The pricing suite covers flat and boundary pricing, whole-session holds, absent
rates/gaps, zero energy/free rates, same-tick energy, subcent aggregate rounding,
eligibility and identity isolation, duplicates/reordering, pauses, rate revisions,
late readings and overflow. Real calendar/DST expansion and monthly statement
revisions still need separate tests when those features exist. Test logs remain
under the existing `target/spec1-test-results/` artifact path for compatibility.

Local verification on 2026-10-02 for the additive pricing/preview changes:

- Rust formatting, Clippy with warnings denied and the complete wrapper passed:
  42 tests, zero failed/ignored/filtered, including all 17 original scenarios.
- Strict OpenSpec validation passed all three active changes. Security guard
  fixtures passed. The frontend TypeScript/production build passed and its npm
  audit reported zero known vulnerabilities at verification time.
- Chrome through agent-browser loaded the local Vite page and Rust API. Desktop
  1440x1080 and mobile 390x844 screenshots were visually inspected in light/dark
  themes. A full-page background seam and disclosure focus spacing were corrected.
- Vehicle filtering returned Car B's $8.80/38 kWh; complete boundary details showed
  4 kWh at $0.20 and 6 kWh at $0.40. Missing evidence returned $5.60/28 kWh for
  Car B and $17.20/80 kWh combined, with an explicit held-session explanation.
  Restoring complete evidence returned $20.40/90 kWh with six sessions.
- Keyboard Enter opened a native disclosure. The narrow layout had no horizontal
  overflow, including open details; buttons/selects were 40px high and session
  controls were at least 72px. Browser error output was empty; console output
  contained only Vite connection and React development-tool information.
- Synthetic screenshots are retained locally under ignored
  `target/preview-verification/`; they are not public source artifacts.
- Exact staged paths and implementation changes were reviewed. Staged and
  complete-history Gitleaks scans passed with no findings. Both listeners were
  confirmed on IPv4 loopback, and browser resource origins were local only.

These additive changes are local on a separate branch. Historical hosted checks
above verify their recorded commits only. No new hosted result, deployment,
live-car test or accuracy certification is claimed.

### Dark dashboard refinement verification

The later 2026-10-02 refinement replaces the initial light/dark view with a
dark-only Geist view and adds bounded weekly/monthly totals and a sample rate
outlook. The fixtures now share a synthetic hourly clock. Display aliases are
Joseph's Model Y Quicksilver and Evan's Model Y Black, requested by the user;
all energy, costs, battery readings and states remain fictional.

- Rust formatting, Clippy with warnings denied and the complete wrapper passed:
  43 tests, zero failed/ignored/filtered, including the 17 original scenarios.
- The preview checks earlier-month inclusion, prior-month exclusion, per-vehicle
  totals, missing evidence in both periods, replay restoration and rate lookup
  before/at the afternoon boundary and midnight. Empty periods return zero.
- The frontend TypeScript/production build passed. Strict OpenSpec and security
  guard checks passed.
- Desktop 1440x1700 and mobile 390x844 screenshots were inspected. Geist resolves
  locally, headings use weight 400, the background remains near-black even with
  a light system preference, and there is no theme switch or wordmark bar.
- The browser verified Evan's complete week $8.80/38 kWh and month $10.40/46 kWh;
  missing evidence changes these to $5.60/28 kWh and $7.20/36 kWh. Restoring both
  cars returns $20.40/90 kWh and $26.00/118 kWh with eight month-to-date sessions.
- The displayed current/next prices match the API's sample clock and schedule.
  Keyboard Enter opens a session, including the 4 kWh/6 kWh rate split. Mobile
  has no horizontal overflow with details open; controls meet the 40px minimum.
- Both the green status pulse and battery reflection animate in Chrome. Reduced
  motion yields zero animations while preserving the Charging text and fill.
  Browser errors are empty; console output contains only development information.
- The changed source and staged paths were reviewed; staged and complete-history
  Gitleaks scans passed. Both preview listeners remain on IPv4 loopback and all
  browser resources come from the local origin.

The current preview remains local. Real tariff calendars, live vehicle status,
authentication, backend storage and deployment are still outside this implementation.

### Menus, preferences and session layout verification

The subsequent 2026-10-02 UI pass sets the title to ChargeShare, adds three
separate vehicle tab buttons with a white selected state, replaces remaining
native menus with shadcn selects, and adds explanatory tooltips and a small
settings dialog. Session duration/start/stop labels come from the Rust preview's
existing hourly boundaries. Desktop aligns four labeled columns; mobile keeps
two-line rows. Native session disclosures use a rotating down caret.

- The TypeScript/production build, Rust formatting, Clippy with warnings denied,
  and all 43 workspace tests passed after the preview time-label change.
- Keyboard arrows change the selected vehicle and both totals in one associated
  tab panel. Complete and missing-reading scenarios retain their expected scoped
  totals; held sessions still show their time range and explanation.
- Select keyboard navigation and Escape, dialog focus trapping/return, and
  tooltip focus/touch access were verified. A CDP touch event opened the rate
  tooltip, and its trigger references the displayed description.
- Settings persist the default vehicle and animation choice after reload.
  Ordinary tab changes leave the saved default intact. Corrupt or invalid stored
  values fall back safely; a blocked write shows a save failure while retaining
  the in-memory setting. Both the saved motion toggle and OS reduced motion stop
  the charging pulse/reflection without hiding the charging state.
- Desktop 1440x1700 and mobile 390x844 and 320x844 screenshots were inspected.
  Matching grid tracks align desktop labels and rows. Vehicle buttons remain
  at least 40px tall and the narrow layout has no horizontal overflow. Opening
  a row retains the cost breakdown. The final browser session reported no errors.
- Strict OpenSpec validation and security guard tests passed. The final staged
  paths/diff were reviewed, and staged plus complete-history scans found no leaks.

Display preferences are the only browser-persisted values. They are not user
accounts, backend session storage, Google login or Tesla authorization.

### Ambient charging effect verification

The active card now has a softly breathing green border and three CSS light rays.
The production build and strict OpenSpec validation passed. Desktop 1440x1080 and
mobile 390x844 screenshots were inspected; text remains readable with no horizontal
overflow. The unplugged card renders no rays. Browser sampling confirmed changing
halo opacity and ray transforms. Turning animation off stops both new effects;
OS reduced motion stops all charging animations. Emulated reduced transparency
hides the rays and removes the inset glow while retaining the Charging label.
Decorative layers ignore pointer events. Browser errors were empty, and the staged
diff/path review and security scan passed. No dependency, API or Rust change was
needed for this visual refinement.

## Dated sample rates and reimbursement verification

Local verification on 2026-10-03 for PR #3:

- Rust formatting, Clippy with warnings denied and the complete offline wrapper
  passed: 51 tests, zero failed/ignored/filtered. The core source is unchanged.
- New cases cover inclusive/exclusive version dates, gaps without fallback,
  ambiguous versions, preserved decimal precision, unchanged evidence repricing,
  derived date labels, winter selection and held reimbursement.
- Strict OpenSpec validation passed four artifacts. Security guard fixtures and
  the frontend TypeScript/production build passed.
- Desktop 1280x1000 and mobile 375x812 screenshots were inspected. The sample
  reimbursement appears first, batteries are compact on mobile, and session
  details retain version IDs and four-decimal rate displays.
- Joseph's September month subtotal is $21.18 for 72 kWh regardless of the
  selected vehicle. October's unverified winter estimate is $21.02 for the same
  synthetic session shape. The September boundary row shows measured 4 kWh
  off-peak and 6 kWh partial-peak, priced at $3.98.
- Settings selector keyboard navigation loaded the missing-reading scenario.
  Evan's held session shows both periods and an unresolved split with no invented
  allocation; his month shows $9.99 for 36 priced kWh. Joseph's top amount stays
  $21.18. Scenario controls are absent from the main dashboard.
- Mobile Settings, winter and held-session screenshots have no horizontal
  overflow. Sample update times and charging energy are explicitly labeled.
  Screenshots remain local under ignored `target/preview-pr3/`.

These figures verify sample calculations only. The winter estimate is not an
account-specific quote; the adapter does not implement UTC or daylight saving.
Hosted checks must be verified on the exact pushed commit independently.

### Dated-rate requirement coverage

The PR #3 completeness pass specifies existing behavior without changing source,
fixtures or dependencies. Each requirement has one implementation task. The
following checks supplement the preceding verification records:

| Requirement / scenarios | Evidence |
| --- | --- |
| Dated rate table: inclusive/exclusive dates and overlapping versions | Preview `date_boundaries_select_only_the_covering_version` includes duplicate covering versions and requires an error. |
| Dated rate table: uncovered dates and hours | Preview `winter_boundaries_and_gap_never_reuse_a_neighbor`, `unchanged_evidence_can_be_repriced_or_held_without_rate_fallback`, and core `absent_rates_and_gaps_never_fall_back_to_a_nearby_rate` verify no adjacent fallback and explicit holds. |
| Dated rate table: precision, public windows and replay | Preview `sample_windows_keep_all_decimal_places` and `unchanged_evidence_can_be_repriced_or_held_without_rate_fallback`; core `boundary_readings_preserve_energy_and_explain_each_rate_and_version`. |
| Bounded sample totals and rate outlook | Preview `week_and_month_use_core_quotes_and_exclude_prior_month`, `missing_reading_is_excluded_from_both_periods_and_replay_restores_totals`, `labels_follow_fixture_clock_boundaries_and_preserve_rate_precision`, and `sample_rate_lookup_switches_at_boundary_and_handles_midnight`. Prior motion checks above still apply. |
| Expanded provenance and four-decimal display | Desktop browser expanded Joseph's September 11 session: `ev2a-summer-2026`, 4 kWh at $0.2774/kWh, 6 kWh at $0.4784/kWh, $3.98 total. Exact arithmetic is separately covered by the Rust tests. |
| Accessible menus and local preferences | Cleared only the owned test browser's preference key and reloaded: Joseph selected. Set Evan through Settings using keyboard selection and reloaded: Evan selected. Ordinary Both cars tab selection left the stored Evan default intact. |
| Sample controls in Settings | No scenario selector on the closed main page. Loaded missing readings from Settings: unresolved split appeared. Reload restored complete September while preserving Evan's default. Prior selector dismissal, storage fallback and tooltip checks above still apply. |
| Sample reimbursement | Preview `winter_preview_uses_its_own_version_and_labels`, `tou_splits_use_measured_lines_and_do_not_invent_missing_allocations`, and `missing_coverage_retains_sessions_and_holds_reimbursement`. Browser Joseph/Evan/Both cars views retained Joseph's $21.18. Card and session footer disclosed fixed-charge/credit exclusions. |
| All-held reimbursement and unavailable outlook | Browser-only synthetic response override with four held Joseph sessions and zero priced energy showed "Needs review", "4 sessions excluded. This amount is incomplete.", and both prices as "Rate unavailable". |
| Sample vehicle context | Desktop 1280x1000 and mobile 375x812: both last-updated lines marked `(sample)`; charging vehicle alone showed `6 kWh added so far (sample)`. Mobile money card preceded 138px/163px vehicle cards with no horizontal overflow. |
| Session time-of-use context | Preview `tou_splits_use_measured_lines_and_do_not_invent_missing_allocations`; browser complete September 11 showed 4/6 kWh split, missing-reading row showed both periods and "Split unresolved" without an allocation, absent coverage showed "Rate unavailable" and "Needs review". |

The all-held browser override checks rendering only; the existing Rust
missing-coverage test separately verifies construction of held results. The
override and temporary preferences were removed afterward. Desktop, mobile and
all-held mobile screenshots were visually inspected; no browser errors occurred.
Evidence is local under ignored `target/preview-pr3/round3-*.png`. These are manual
browser checks, not an automated screenshot regression suite. Full local checks
passed again: 51 Rust tests, formatting, Clippy, strict OpenSpec validation,
security guards and the TypeScript/production build.

## Future receiver integration: separate spec

Proposed path: fake vehicle → actual official Tesla Go receiver → selected
supported dispatcher/broker → Rust adapter → vehicle ledger. This tests transport
and decoding as well as accounting; a direct JSON injection into Rust is not an
end-to-end receiver test. Broker selection and adapter details are still open.

Carry this checklist into the later integration spec before building it:

- [ ] Select the dispatcher and pin the official receiver to a reviewed immutable commit or image digest; pin compatible protocol tools, dependencies and container images, and document the exact reproducible local command
- [ ] Adapt Tesla's test client into deterministic, multi-vehicle AC charging fixtures with distinct identities, event IDs, timestamps, counters and state transitions; verify the same 10 kWh / 4 kWh ledger expectations across the real receiver boundary
- [ ] Start the actual Go receiver, selected dispatcher and Rust consumer in isolated local containers on a fresh GitHub-hosted runner; generate ephemeral test-only CA/server/client certificates, wait for health checks and exercise authenticated WSS without any Tesla endpoint or account
- [ ] Test duplicate/out-of-order evidence, invalid/unknown vehicles and restart/reconnect behavior under the selected dispatcher's delivery semantics; assert ledger isolation, no double counting and explicit incomplete-data flags
- [ ] Add a GitHub Actions integration job on every pull request and push, with bounded startup/test timeouts and cleanup even on failure; assert that untrusted test certificates are rejected; prove unexpected handshake failures, unavailable dependencies and failed assertions fail the job rather than skip it
- [ ] Verify the full suite from a clean runner with no developer state or repository secrets; publish a safe result summary and allowlisted failure diagnostics, then record a passing final-commit run before calling the integration complete

Tesla's Fleet Telemetry test client (`test/integration/server_test.go` upstream)
constructs protobuf payloads inside FlatBuffers messages and uses WSS with local
certificates. Its example payload contains name/location fields, not a complete
charging simulation. Adaptation must replace those fields with synthetic charging
evidence and omit location. The upstream `Makefile` provides certificate-generation
and container integration entry points. A later approved integration spec must
record the reviewed revision and exact test setup; these upstream examples do not
test ChargeShare or authorize work beyond the current offline suite.

## CI isolation and failure evidence

The offline suite and any future receiver suite must fail their CI job on failure; do not mask failures
with `continue-on-error`. Keep human-readable test counts/results in the run
summary and upload only allowlisted synthetic diagnostics when useful, including
on failure. The offline artifact action is pinned with seven-day retention; preserve those
limits for future suites. A deliberate assertion failure followed by a restored passing
run demonstrates the gate. Requiring these checks in branch protection is a
separate repository setting; this document does not configure it.

Test certificate trust is isolated to the disposable test network. Never add the
test CA to production trust, disable TLS verification, use production keys,
configure OAuth, pair a real vehicle, expose a public listener or access Tesla
accounts from CI. Generate private test keys only in temporary runtime storage;
do not commit them or include keys, certificates, credential files or unrestricted
container dumps in uploaded artifacts. Clean up containers and temporary state
on success and failure. Dependency/image downloads during setup are distinct
from the isolated test traffic itself.

## Manual real-car validation gate

A future, separately approved supervised trial must confirm actual vehicle signal
support, counter/reset semantics, session boundaries, final samples and the
[measurement boundary](architecture.md#measurement-boundary). Observe at least
five supervised sessions over approximately one week, including pause/resume and
a time-of-use boundary if applicable. Privately record firmware, telemetry
configuration, baseline/end readings, disconnects and unresolved gaps. Agree
private evidence handling first; obtain permission for any independent AC meter
reference and agree a comparison tolerance. Without a reference, retain the
unvalidated-accuracy label and agree that limitation before reimbursement.

Retain flags for missing baseline/terminal sample, rollback/reset ambiguity,
invalid signal, long gaps, ambiguous AC/DC state, unconfirmed charger, estimated
tariff allocation and unvalidated physical accuracy. Later pricing tests must
cover flat rates, time-of-use splits, DST, month-end, tariff revisions, rounding,
replay without double-counting and long cross-rate gaps that block finalization.
Synthetic examples: 10 kWh at 0.20 currency units/kWh is 2.00 units; 4 kWh at
0.20 plus 6 kWh at 0.40 is 3.20 units before separately agreed charges. Only
reviewed, complete AC shared-charger sessions may enter a later payable total.
These are future acceptance requirements, not implemented billing behavior.
Real charging, OAuth/key pairing, deployment and costs are not
automated CI tests and are not authorized by this plan. Passing simulated tests
proves software behavior for those fixtures, not utility-meter accuracy.

## Local Nix/Tilt transport implementation checks

Stage 1 implementation on 2026-10-04 adds an outer Rust synthetic harness without
changing `chargeshare-core` APIs or dependencies. The workspace now has 61 tests:
51 existing core/preview tests plus ten fixture/verifier/deadline tests. Local
formatting, Clippy with warnings denied, the complete offline wrapper, strict
OpenSpec validation, security guard tests and the frontend production build pass.
The frontend cache was project-local; original dependency locks remain intact
apart from the approved MIT metadata. Generated license copies are included in
`dist/licenses`. Runtime helper tests additionally exercise daemon/configuration
failures, occupied ports, reset ownership and real disposable loopback TLS trust.

The Nix flake evaluates, its genuine lock remains stable on repeated resolution,
and pinned Docker/Compose/Tilt/Go CLI versions were checked from official cached
outputs. Pinned Tilt evaluated the actual Tiltfile successfully without starting
services. The original `npm run preview:dev` command returned successful frontend
and Rust proxy API responses, and its shutdown released both preview ports. This editing sandbox has no host Docker daemon or installed Nix store.
It has not run full shell entry, container builds or a receiver/Kafka round trip.
Those acceptance claims remain pending the exact-commit
[inactive integration workflow](workflows/telemetry-integration.yml.disabled),
which needs [maintainer activation](workflows/README.md) before it can run.
No later Cloudflare, SQLite, ingestion or live dashboard coverage is implied.

Rechecked on 2026-10-07 with Rust 1.99.0 on Linux x86_64: all 61 workspace
tests, 51 runtime tests, formatting, Clippy, strict specs, security guards/scans
and the frontend build pass. The direct preview serves the frontend and proxied
fixture API, then releases both listeners on shutdown. Four ACK regression
tests enforce a total deadline across WebSocket keepalives and continuous partial
input. A cancellable watchdog closes the socket at the 10-second deadline and
is joined before return. The Docker/Nix
prerequisites are still absent in this sandbox, so full transport and lifecycle
tasks remain unchecked.
