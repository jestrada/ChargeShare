# session-pricing Specification

## Purpose

Explain the cost of eligible synthetic AC charging evidence without concealing
missing timing, rate uncertainty, or existing session exclusions.

## Requirements

### Requirement: Exact USD arithmetic
The system SHALL accept nonnegative decimal USD-per-kWh rates with up to ten
fractional digits, multiply by exact energy, detect overflow, and round the
aggregate priced subtotal once to cents using half-up rounding.

#### Scenario: Flat rate
- **WHEN** a complete confirmed session records 10 kWh wholly within a 0.20 USD/kWh window
- **THEN** its exact cost and rounded subtotal are 2.00 USD

#### Scenario: Subcent aggregation
- **WHEN** two eligible sessions each cost exactly 0.004 USD
- **THEN** the aggregate rounds to 0.01 USD rather than summing rounded session costs

#### Scenario: Invalid and excessive input
- **WHEN** a rate is signed, malformed, non-finite, has excess precision or exceeds its representation
- **THEN** pricing configuration fails with a safe reason without echoing input
- **AND** arithmetic overflow fails rather than wrapping or returning a partial subtotal

### Requirement: Explicit versioned rate windows
The system SHALL require valid, nonoverlapping half-open rate windows in the same
synthetic tick domain as events. Each window SHALL carry a synthetic version
identifier and rate. Window order SHALL not affect results.

#### Scenario: Boundary samples
- **WHEN** counter readings establish 4 kWh before a rate boundary at 0.20 and 6 kWh after it at 0.40
- **THEN** the exact cost is 3.20 USD with two lines retaining their rates and versions
- **AND** a segment ending at the boundary uses the preceding window

#### Scenario: Invalid tariff
- **WHEN** windows overlap, have zero or negative duration, or use an invalid version identifier
- **THEN** configuration is rejected before quoting a session

### Requirement: Conservative interval pricing
The system SHALL derive positive counter segments from the ledger's evidence.
Every positive segment MUST fit entirely inside one rate window with increasing
time. Otherwise the entire session SHALL be held with no partial quote. Zero
energy SHALL not be assigned a made-up cost or rate.

#### Scenario: Missing boundary reading
- **WHEN** a 10 kWh counter interval crosses two rate windows without a boundary reading
- **THEN** the session is held for an unresolved rate boundary
- **AND** no start-rate pricing or time-proportional interpolation occurs

#### Scenario: Missing rate or time
- **WHEN** a positive segment starts outside rate coverage or has equal start and end ticks
- **THEN** the session is held with the applicable explicit reason

#### Scenario: No consumption
- **WHEN** a complete confirmed session has unchanged counters
- **THEN** it can receive a zero quote with no cost lines, even without rate windows

### Requirement: Eligibility and scope are preserved
The system SHALL quote only sessions already eligible under the ledger contract.
It SHALL return held and excluded sessions with their existing reasons and retain
vehicle and owner scope. Unknown vehicles SHALL fail without mutation.

#### Scenario: Excluded or incomplete evidence
- **WHEN** a session is unconfirmed, belongs to another charger, contains DC or ambiguous evidence, or has any quality flag
- **THEN** it contributes no priced energy or cost and its reasons remain visible

#### Scenario: Two owners
- **WHEN** two registered vehicles have interleaved evidence and independent charger reviews
- **THEN** each scoped query returns only its own sessions, owner and subtotal

### Requirement: Reproducible review results
The system SHALL return exact per-line costs, exact per-session costs, a priced
energy subtotal, a once-rounded cost subtotal, and visible held sessions. It MUST
retain the synthetic, physically unvalidated evidence label and SHALL NOT present
the result as a monthly invoice or payment authorization.

#### Scenario: Duplicates and late boundary evidence
- **WHEN** evidence is reordered or duplicated
- **THEN** pricing remains deterministic and no energy is counted twice
- **WHEN** a valid late boundary reading resolves a held interval
- **THEN** a new query produces a resolved quote while the previous returned result remains unchanged

#### Scenario: Offline execution
- **WHEN** the fictional demonstration and acceptance suite run
- **THEN** no vehicle account, real identifier, credential, network service or private file is required

### Requirement: Dated rate table
The system SHALL keep rate versions separate from charging evidence, with an ID,
inclusive effective-from date, exclusive effective-until date, daily TOU windows
and exact decimal USD/kWh strings. Only the version covering the requested date
SHALL supply windows to the existing pricing rules. Ambiguous coverage MUST fail.

#### Scenario: Effective date and gaps
- **WHEN** a date reaches a version's start or exclusive end
- **THEN** only a version covering that date supplies rates
- **AND** uncovered positive consumption remains held without borrowing earlier or later rates

#### Scenario: Overlapping versions
- **WHEN** two versions cover a requested date
- **THEN** schedule creation fails rather than choosing one or returning partial coverage

#### Scenario: Uncovered hour
- **WHEN** a positive consumption interval is outside every window on a covered date
- **THEN** the session is held for a missing rate and excluded from priced totals
- **AND** no nearby window supplies a fallback rate

#### Scenario: Reprice unchanged evidence
- **WHEN** the same evidence is replayed with an updated dated table
- **THEN** energy and eligibility are unchanged and costs use only the selected exact rates
- **AND** replay with the same table remains deterministic

#### Scenario: Public sample
- **WHEN** the committed September historical sample is used
- **THEN** `ev2a-summer-2026` covers September 1 through September 30, 2026
- **AND** daily half-open windows are 00:00-15:00 off-peak at `0.2773931020`, 15:00-16:00 and 21:00-24:00 partial-peak at `0.4783693788`, and 16:00-21:00 peak at `0.5866379696` USD/kWh
- **AND** it is labeled as a sample public tariff without private identifiers or bill contents

#### Scenario: Winter estimate
- **WHEN** a date is October 1, 2026 through May 31, 2027
- **THEN** `ev2a-winter-2026-est` supplies `0.2779120234`, `0.4465438926` and `0.4624755018` with the same daily windows
- **AND** June 1 has no inherited winter rate
