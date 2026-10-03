# Design

## Context

See [proposal](proposal.md) for scope and motivation. On merged main, the core
already validates immutable `RateSchedule` windows and prices retained counter
segments with exact arithmetic. Windows and events use synthetic i64 ticks, not
Unix timestamps. `demo.rs` maps these ticks to September sample dates and builds
fictional rates inline. Its `rate_outlook` currently errors if either rate is
missing and its `rate_label` rounds the per-kWh value to cents.

The preview has `serde_json` and `tiny_http`; the core has no runtime dependencies.
There is no database, tariff import, real calendar or persistence to migrate.

## Goals / Non-Goals

**Goals:** make a small rate adapter feed the existing pricing engine; preserve
unchanged evidence across profile revisions; keep absent coverage visible; let
the local UI explain exactly which supplied rates it used.

**Non-goals:** interpreting provider telemetry time, changing the core clock,
supporting all utility calendars, building an editable rate-management UI or
maintaining a permanent revision/audit database. A source label records an
operator's description, not an assertion of tariff verification by the software.

## Decisions

### 1. A date-bounded file with daily whole-hour windows

Schema version 1 has `schema_version` and `versions`. Each version has:

| Field | Meaning |
| --- | --- |
| `id` | Unique 1-64 character ASCII alias accepted by the core rate-window type |
| `effective_from` | Inclusive date in strict `YYYY-MM-DD` form |
| `effective_until` | Exclusive date in the same form; a finite end is required |
| `source_label` | Nonempty plain text, at most 120 UTF-8 bytes, no control characters |
| `daily_windows` | 1-24 entries covering hours 0 through 24 exactly once |

Each daily entry contains integer `start_hour`, integer `end_hour`, and a
`usd_per_kwh` decimal string. Exact parsing delegates to `UsdRate`; JSON floating
point values are not accepted for money. Use the existing ID character rules,
reject unknown fields and duplicate object members, and sort valid date ranges
before checking overlaps. Between 1 and 24 versions and at most 64 KiB of source are accepted.
Dates must be real Gregorian dates in years 2000-2099, with start before end.

For example, fictional v1 can cover September 1-10 at 0.20 USD/kWh and v2 can
cover September 10-15 at 0.30. Changing an old rate is represented by supplying a
replacement profile with a new ID for the corrected version; unchanged later
dates retain their own version. No in-place mutation of returned quotes occurs.
There is no durable registry to detect ID reuse across separate process launches,
so documentation must require new IDs for changed content and retention of prior
files when reproducible historical comparisons matter.

An undated hardcoded override is smaller but loses historical applicability.
A database or tariff editor adds state, migration and account decisions that this
preview does not need. Complete daily windows make accidental hourly omissions
configuration errors; gaps between dated versions remain valid missing coverage.

### 2. Resolve dates outside the core

Add a cohesive rate-profile module under `chargeshare-preview`. It owns validated
profile values and deterministic compilation; startup owns file IO. Existing core
`RateWindow` and `RateSchedule` validate the resulting schedule. No serde, file IO
or date processing enters `chargeshare-core`, and no public core API changes.

Compile only the fixed sample horizon September 1 through September 14 inclusive.
For day d, the existing mapping remains `d * 24 + hour`. Date intervals outside
that horizon produce no windows; they are never stretched to fill missing days.
The extra day supports the next-window outlook at the sample clock. Prior-month
fixtures remain outside displayed sample totals as they already are.

Adjacent windows with the same version and exact rate can be coalesced to avoid
inventing a daily pricing discontinuity. Never coalesce across version IDs or
coverage gaps. A true version/rate boundary retains the core's conservative hold
when the counter evidence cannot establish energy on each side, even if two
different versions happen to contain equal numeric rates.

All profile dates are explicitly declared sample civil dates. They do not encode
the host timezone. This fixed September mapping has no daylight-saving transition
and does not justify a generic timezone library or changes to ledger events.
A future live adapter must explicitly establish real timestamp/timezone semantics
and review DST cases before applying these profiles to real sessions.

### 3. Startup-only selection and safe failure

Extend the launcher and preview executable with an optional `--rate-file PATH`.
Without it, preserve the exact existing default. With it, open the selected
regular file, read no more than 64 KiB plus one sentinel byte, and validate before
binding the API. Invalid input returns a fixed reason code without content or
path; it cannot silently substitute fictional prices. Reload requires a restart.

Keep private profiles outside the checkout. No directory scanning, environment
discovery, upload route, file-serving route or rate editing in browser storage is
introduced. Browser responses expose only needed rates and provenance, not raw
source JSON or file paths. Render source labels as plain text. Both preview
listeners retain loopback binding and existing request restrictions.

Use the existing JSON tooling. If typed strict decoding needs a direct `serde`
dependency, pin the compatible version already in the lockfile in the preview
crate; do not add a runtime dependency to the core. A web upload would introduce
a larger data-handling surface with no benefit to this local experiment.

### 4. One schedule for quotes and outlook

Pass the selected immutable schedule into the existing demo snapshot. Reuse the
same event fixtures and scoped core queries. Add source metadata at the response
boundary, including effective dates and per-line version/source lookup. Return
explicit unavailable outlook values rather than converting missing rates into an
API failure. Only a contiguous immediately next window counts as the next price.

Local mode labels the rate card "Price at sample time" and retains the frozen
clock. Cost totals explicitly describe sample sessions priced with locally
supplied rates. The detailed rate string uses exact decimal formatting, trimming
only insignificant trailing zeros. A compact rate may round to five decimal
places with an approximation label; calculation always uses the original value.
Existing dark styling, filters, session disclosures and preferences remain.

## Risks / Trade-offs

- Historical figures mistaken for actual spending: persistent demo labels,
  sample-clock wording, rate provenance and a statement that totals are not owed.
- Unsupported dates mistaken for a live tariff calendar: limit schedule expansion
  to the documented sample horizon and show missing coverage explicitly.
- Combined rates omit or duplicate bill components: rate reconstruction and
  verification occur privately before loading; no automatic fee, credit, tax or
  loss adjustment is performed by this adapter.
- File revisions lose an audit trail: version IDs travel with quotes; keep prior
  profiles externally. Durable snapshots/settlements are outside this milestone.
- Earlier dashboard specifications only describe fictional input: the proposal
  explicitly scopes the local-file exception to rate configuration; the canonical
  synthetic vehicle-ledger contract and default dashboard behavior stay intact.

## Migration Plan

There is no deployment or database migration. Ship preview API, launcher and UI
changes together after scope approval. Synthetic tests cover both default and
local modes, invalid inputs, gaps, date/rate boundaries, exact precision and
unchanged evidence. Verify desktop/mobile states with a synthetic local profile.
Removing the startup flag restores the original demo. Household rate validation
and any actual local profile stay outside public commits and screenshots.
