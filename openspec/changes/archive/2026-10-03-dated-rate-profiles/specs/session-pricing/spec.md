## ADDED Requirements

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
