# Offline pricing contract

This milestone uses fictional charging evidence and fictional, already resolved
rate windows. It does not connect to Tesla or certify physical energy accuracy.
The [pricing change](../openspec/changes/offline-session-pricing/proposal.md)
records scope; its task list distinguishes implemented work from pending work.

## Exact amounts

`UsdRate::parse_per_kwh` accepts unsigned ASCII decimals with a whole part and
at most ten fractional places. Signs, whitespace, exponents, non-finite values,
empty fractions and excess precision are errors; values are never truncated.
The largest rate is 1,844,674,407.3709551615 USD/kWh. Zero is valid.

`UsdRate::cost_for(Energy)` multiplies exact six-place kWh by the rate to produce
`ExactUsd`, with sixteen decimal places and u128 storage. The single product of
two bounded u64 inputs cannot overflow; `ExactUsd::checked_add` reports
`MoneyError::CostOverflow` when the sum cannot be represented. Errors contain
reason codes, not rejected source strings.

`ExactUsd::round_half_up_to_cents` returns `UsdCents`, whose `as_cents` accessor
exposes integer cents. Half a cent rounds up. Aggregate exact values first;
two costs of 0.004 USD total 0.008 USD, which rounds to 0.01 USD. Rounding each
first would incorrectly produce 0.00 USD. No floating-point arithmetic is used.

## Rate windows

`RateWindow::new(version, start_time, end_time, rate)` validates a nonempty
half-open interval `[start_time, end_time)` and a 1–64 character synthetic version
alias using ASCII letters, digits, underscores or hyphens. Its getters expose
the version, bounds and `UsdRate`; the validated fields cannot be mutated.

`RateSchedule::new(windows)` sorts by start time and rejects overlaps, including
duplicate windows. Adjacent windows and gaps are permitted. An empty schedule
provides no positive-energy coverage. `windows()` returns the ordered slice.

These i64 bounds are the ledger's synthetic ticks, not Unix timestamps or local
clock hours. No timezone, DST, seasonal proration, month selection or real utility
rate calendar is inferred. A later adapter must resolve those semantics. Rates
are caller-supplied combined variable USD/kWh values; the engine adds no fixed
fee, credit, tax or charging-loss multiplier. Utility component reconstruction
and a versioned source registry remain separate work.

## Session quotes and priced subtotals

`Ledger::pricing(&vehicle, &schedule)` is a read-only, explicitly vehicle-scoped
query. Unknown vehicles return `PricingError::Ledger(UnknownVehicle)`. It uses
the same reconstruction as `sessions()`; every positive counter delta retains
its earlier and later sample ticks internally. It never reconstructs energy
from battery percentage, elapsed time, session totals or a requested price.

`PricingSummary.sessions` includes every session, each carrying the original
Session and `quote: Result<SessionQuote, PricingHold>`. Existing quality flags,
charger classification, exclusions, owner identity and synthetic evidence label
are retained. Scope checks remain domain invariants, not user authentication.

A quote is available only for an eligible session whose every positive energy
segment lies inside a single rate window. Lines contain exact energy and cost,
sample ticks and the complete applicable rate window. A segment ending exactly
at a window's end uses that window; one beginning there uses the next window.
No positive segment may have equal start and end ticks.

The first chronological pricing problem holds the entire session:

| Hold | Meaning |
| --- | --- |
| `IneligibleSession` | Existing quality or charger exclusions prevent pricing; inspect Session reasons. |
| `MissingRate` | No rate covers the positive segment's start. |
| `UnresolvedRateBoundary` | The segment crosses the end of its starting rate window, including a coverage gap. |
| `NonIncreasingTime` | Positive energy has no increasing time interval. |

Timing/rate holds include the affected interval. Adjacent equal-rate windows
are still distinct version boundaries; this conservative milestone requires a
sample at that boundary too. Zero counter deltas create no lines and require no
rate. A complete confirmed zero-consumption session can have a zero quote even
with an empty rate schedule. Other eligibility rules still apply.

`priced_energy` and `exact_priced_subtotal` include only fully quoted sessions.
`rounded_priced_subtotal` rounds that exact aggregate once. Check
`has_unpriced_sessions()` and display session holds alongside the subtotal.
For example, if one session has an unresolved split and a separate 1 kWh session
is covered by 0.20 USD/kWh, the priced subtotal is 0.20 USD with the first session
still held. It is not a total amount owed. Empty registered vehicles return an
empty zero subtotal; an unknown vehicle is an error.

Arithmetic errors abort the query without a partial result or ledger mutation.
Duplicate and reordered evidence produces identical quotes. A late boundary
reading or a different supplied schedule can change the next query's result;
previous returned snapshots are owned values and are not automatically updated.
These snapshots do not implement persisted statement revisions or invoices.

## Run the example

```sh
cargo run --example offline_pricing --locked
```

Vehicle A has 4 kWh at a fictional 0.20 USD/kWh and 6 kWh at 0.40 USD/kWh:
10 kWh and 3.20 USD are priced. Vehicle B has the same total energy without
the boundary reading: the example displays its explicit hold and no priced
amount. All dates, times, identities, readings and rates are fictional.
