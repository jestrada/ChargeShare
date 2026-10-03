# Design

## Context

See [proposal](proposal.md). The current core stores evidence in a vehicle-scoped
Ledger, reconstructs sessions in `(time, position)` order and has exact six-place
energy. Session totals alone cannot establish time-of-use allocation.

## Goals / Non-Goals

Preserve the existing public Session and Summary shapes and Spec 1 behavior.
Price only evidence-derived intervals, through a new scoped query. This additive
library capability introduces no runtime dependency or persistent format.
Resolved synthetic rate windows deliberately precede a separate real timestamp,
calendar tariff and statement integration; no claim of DST or monthly billing
support is made.

## Decisions

- Keep `lib.rs` a facade. `money` owns exact rate/cost arithmetic, `tariff` owns
  validated windows, `pricing` owns quotes/holds/subtotals and its safe errors.
  Monetary errors live with money and tariff errors with tariff to avoid cycles.
  Session reconstruction has no dependency on pricing. Ledger orchestrates both.
- Extend the internal reconstruction result with counter segments collected at
  the same point as existing energy deltas. Keep public Session unchanged.
  Extract the existing Ledger reconstruction loop into a shared private method;
  `sessions` maps to the existing output and `pricing` consumes the same evidence.
  A second replay implementation or caller-provided session totals could drift
  from eligibility and is unnecessary.
- Represent rates as checked u64 units of 10^-10 USD/kWh and exact costs as u128
  units of 10^-16 USD. A product of two u64 values fits u128; aggregate addition
  remains checked. Round by quotient/remainder to avoid adding an overflowing
  half-unit. USD is explicit; floating-point and a generic currency framework
  add risk or scope without a consumer.
- Immutable validated rate windows contain start/end ticks, a version alias and
  a combined variable rate. Sort windows at construction and reject overlap.
  Retain a clone of the applicable window on every quoted line. This is enough
  provenance for fictional review; a later tariff importer owns component
  calculation, timezone expansion, effective dates and source documents.
- Positive counter intervals must fit one entire window. Adjacent windows even
  with equal rates remain distinct evidence boundaries. With no matching start
  window hold for missing rate; crossing its end holds for unresolved boundary.
  No partial session quote, interpolation or generic loss multiplier is allowed.
  Zero deltas produce no line and need no tariff. Same-tick positive deltas hold.
- Return a SessionPricing containing the original Session plus either a complete
  SessionQuote or an explicit PricingHold. PricingSummary exposes only the priced
  subset and whether unpriced sessions exist. Sum exact costs across all quoted
  sessions before one half-up cent rounding. It is not a finalized statement.

## Risks / Trade-offs

- Synthetic ticks resemble timestamps → document units as unspecified and reject
  any claim that this expands Pacific time, DST, seasons or month boundaries.
- A valid cumulative delta can hide timing uncertainty → require an enclosing
  single rate window; hold the entire session otherwise.
- Unpriced sessions could look like zero spending → include every session result
  and name the aggregate a priced subtotal, never a total amount owed.
- Internal replay changes could alter existing energy behavior → retain all
  existing acceptance tests unchanged and add independent pricing scenarios.
- An immutable returned snapshot is not persisted audit history → explicitly
  defer statement revisions/storage and do not issue invoice IDs.

## Verification

Test flat and boundary pricing, subcent aggregate rounding, numeric limits,
unsafe rate inputs, invalid windows, same-tick positive deltas, zero energy,
missing rates, whole-session holds, exclusions, two vehicles, duplicate/reverse
ingestion and late boundary readings. Run all Spec 1 tests unchanged, formatting,
Clippy, strict OpenSpec validation and security guards. Extend the existing CI
wrapper to require the new acceptance suite as well as the old suite. Run the
fictional example locally. No new hosted run is claimed without publication.
