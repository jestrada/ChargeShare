# Dated rate profiles

## Purpose

Apply explicitly dated, locally supplied electricity rates to sample charging
evidence while keeping evidence, rate provenance and cost uncertainty separate.

## ADDED Requirements

### Requirement: Explicit local rate configuration
The preview SHALL use fictional rates by default and SHALL load a rate-only file
only when explicitly selected at startup. Invalid selected configuration MUST
prevent startup with a safe reason. Loading SHALL be bounded and SHALL NOT add
HTTP file access, uploads, private-file discovery or external network requests.

#### Scenario: Default invocation
- **WHEN** the preview starts without a rate file
- **THEN** it reads no private rate file and retains the existing fictional totals, scenarios and rate outlook

#### Scenario: Invalid selected file
- **WHEN** the selected file is unavailable, exceeds 64 KiB, is not a regular file, has malformed JSON, duplicate object keys, unknown fields or an unsupported schema version
- **THEN** startup fails without binding the preview API or silently falling back to fictional prices
- **AND** the diagnostic does not include file contents, source labels or the selected path

#### Scenario: Rate files remain local
- **WHEN** the local mode is exercised or verified
- **THEN** no bill, account identifier or real vehicle event is imported
- **AND** the public repository and automated fixtures contain synthetic profiles only
- **AND** an HTTP request cannot select, upload or retrieve a source file

### Requirement: Date-bounded versions
A profile SHALL contain 1-24 versions with unique IDs, source labels and valid
start-inclusive, end-exclusive civil dates in years 2000-2099. Versions SHALL NOT overlap. Each version SHALL define
a complete nonoverlapping daily set of whole-hour windows and exact nonnegative
USD/kWh decimal rates with at most ten fractional digits. Date gaps SHALL remain
uncovered rather than inheriting an earlier or future version.

#### Scenario: Effective date selects the version
- **WHEN** fictional v1 ends on September 10 and v2 starts on September 10
- **THEN** September 9 uses v1 and September 10 uses v2
- **AND** reversing their input order produces the same selection

#### Scenario: Reject ambiguous configuration
- **WHEN** dates are invalid or reversed, version IDs repeat, date ranges overlap, daily windows overlap or omit hours, or a rate is signed, non-finite or too precise
- **THEN** the profile is rejected before any quotes are returned

#### Scenario: No rate outside its dates
- **WHEN** a sample date falls in a date gap or after the final effective end
- **THEN** it has no applicable rate, even if another version has the same daily window

### Requirement: Evidence-preserving repricing
Repricing SHALL preserve session identity, energy, classification and quality.
Each cost line SHALL retain its rate version and exact rate. Existing whole-session
holds, vehicle isolation and aggregate-before-rounding rules SHALL remain.
Neither rate selection nor a new version SHALL infer missing boundary energy.

#### Scenario: Update rates without rewriting events
- **WHEN** the same complete 10 kWh synthetic session is quoted with a 0.20 profile and then a corrected 0.30 profile with a new version ID
- **THEN** the quotes are respectively 2.00 and 3.00 USD and all underlying session evidence is identical
- **AND** an already returned quote retains its original rate and cost

#### Scenario: Version boundary with insufficient evidence
- **WHEN** a positive counter interval crosses an effective-date boundary without a reading at that boundary
- **THEN** the whole session is held with a visible reason and excluded from priced totals
- **AND** energy is not divided by elapsed time or assigned to the start rate

#### Scenario: Missing rate and independent vehicle
- **WHEN** one vehicle has positive energy outside rate coverage and another has a complete eligible session within coverage
- **THEN** the first session is held while the second remains priced in its own scoped totals

#### Scenario: Repeat and restore
- **WHEN** unchanged evidence is replayed with duplicates or shuffled arrival order against unchanged versions
- **THEN** session and cost results are identical
- **WHEN** a previous profile is selected again
- **THEN** its original quote results are restored without altering charging evidence

### Requirement: Honest historical preview
Local mode SHALL keep the frozen sample clock and demo vehicle/session labels.
It SHALL show that historical rates price sample sessions, identify rate sources
and versions, and state that totals are not actual spending or amounts owed.
It SHALL NOT claim that supplied rates are independently verified or current.

#### Scenario: Historical profile in the dashboard
- **WHEN** local rates are loaded
- **THEN** the rate section shows the sample date/time, applicable source and effective dates
- **AND** expanded priced sessions show the source/version and exact supplied rate with up to ten decimal places
- **AND** weekly/monthly totals come from the same selected schedule and retain existing sample-period definitions
- **AND** fixed fees, credits and loss adjustments are identified as outside these variable-rate totals

#### Scenario: Rate outlook is unavailable
- **WHEN** the sample time has no applicable rate, or the immediately following rate window is uncovered
- **THEN** that part of the outlook says unavailable without failing the entire dashboard
- **AND** a later covered window is not presented as the immediately next price across an undisclosed gap

#### Scenario: Precision and accessibility
- **WHEN** a rate such as 0.1234567890 USD/kWh is supplied
- **THEN** its detailed display preserves that value without rounding it to cents
- **AND** any shortened outlook display is identified as rounded and is never used to calculate costs
- **AND** rate provenance, unavailable states and session details remain readable on mobile and accessible with a keyboard

### Requirement: Bounded sample calendar
This increment SHALL apply profiles only to the documented September 2026 hourly
sample timeline. Effective dates SHALL be explicit calendar dates; core event ticks
SHALL retain their synthetic meaning. No wall-clock, DST, season or utility change
SHALL be inferred from the machine's date, locale or timezone.

#### Scenario: Changing the host clock
- **WHEN** the same preview runs under another host timezone or on another date
- **THEN** its frozen sample times, version selections and costs remain identical

#### Scenario: Future rates do not become current
- **WHEN** a valid profile contains only versions outside the sample horizon
- **THEN** sample positive-energy sessions lack rate coverage and the rate outlook is unavailable
- **AND** the dashboard does not extrapolate those rates into September or claim to price live charging
