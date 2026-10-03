# Design

## Decisions

- Keep a Rust const table in the preview crate: version ID, inclusive/exclusive
  calendar dates, daily hour windows and exact decimal USD/kWh strings. This needs
  no file parser or new dependency; checked-in JSON would add parsing without benefit.
- A small adapter selects by an explicit date and emits existing `RateWindow`s
  into `RateSchedule`. No match emits no windows; ambiguous matches fail. The
  core pricing/hold rules remain untouched, including cross-window holds.
- The demo explicitly associates September fixture days with synthetic hourly
  ticks. This is not UTC, timezone handling or a new Tesla event contract.
- Use one `ev2a-summer-2026` version for September 1 through October 1
  (exclusive). Daily windows are 00–15 at `0.2773931020`, 15–16 and 21–24 at
  `0.4783693788`, and 16–21 at `0.5866379696` USD/kWh. These are the user-supplied
  historical public tariff sample, not a bill or a current account-specific quote.
- Add `ev2a-winter-2026-est` for October 1, 2026 through June 1, 2027
  (exclusive): `0.2779120234`, `0.4465438926`, `0.4624755018`, same windows.
  Bound this unverified estimate to the stated winter season; never extend it
  automatically. An October fixture uses the same synthetic event shape and
  labels all its rates and reimbursement as “Winter estimate, unverified”.
- Replace the default fictional prices. Display rates to four decimal places
  using integer rounding; calculations retain all ten. One expanded-session line
  lists the version IDs already retained by the core quote.
- Derive period labels from `DEMO_NOW`, `WEEK_START` and `MONTH_START` using the
  fixture's selected month. Keep the table separate from immutable events.

## Verification and limits

Test inclusive/exclusive dates, gaps, version changes, preserved precision and
replay of identical evidence under different tables. Check complete/held demo
totals, boundary prices, derived labels and desktop/mobile rendering.
Coverage is explicitly dated; no adjacent rate is a fallback. Live event semantics,
seasonal/current tariffs and connector/hosting choices remain separate work.

The reimbursement card uses Joseph’s exact eligible month subtotal, independent
of the selected vehicle filter. Held sessions remain excluded and counted; no
payment state is implied. Freshness and charging-added energy are labeled fixtures,
not telemetry. TOU labels come from the selected daily windows; unresolved splits
list periods without inventing energy allocations. Saved view preferences remain
respected; a fresh browser defaults to Joseph. Scenario controls use the existing
Settings dialog, with September complete/missing and October estimate fixtures.
