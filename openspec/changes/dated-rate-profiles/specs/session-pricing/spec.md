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

#### Scenario: Reprice unchanged evidence
- **WHEN** the same evidence is replayed with an updated dated table
- **THEN** energy and eligibility are unchanged and costs use only the selected exact rates
- **AND** replay with the same table remains deterministic

#### Scenario: Public sample
- **WHEN** the committed September historical sample is used
- **THEN** its rates are `0.2773931020`, `0.4783693788` and `0.5866379696` with the existing off/partial/peak windows
- **AND** it is labeled as a sample public tariff without private identifiers or bill contents

#### Scenario: Winter estimate
- **WHEN** a date is October 1, 2026 through May 31, 2027
- **THEN** the winter version supplies `0.2779120234`, `0.4465438926` and `0.4624755018` with the same daily windows
- **AND** June 1 has no inherited winter rate
