# Local dashboard preview

## Why

The pricing library can be exercised only through tests and a terminal example.
A local browser view lets contributors review the shared-charger experience and
its uncertain-data states before connecting real vehicles.

## What Changes

- Add a responsive, dark-only shadcn dashboard and a separate Rust preview API.
- Show explicitly fictional battery cards, sessions, priced energy and cost.
- Use the real core ledger/pricing calculation for session totals and holds.
- Allow switching complete/missing-reading scenarios, filtering by vehicle and
  opening per-session cost details with native browser controls.
- Listen only on loopback and use bundled assets with no external services.
- Follow the later requested full glass direction and battery-widget references:
  near-black surfaces, restrained Geist typography, active green charging and larger battery readings.
  Remove the wordmark/header and theme selector; keep simple copy and accessible controls.
- Use the user-requested Joseph/Quicksilver and Evan/Black display aliases with
  wholly synthetic readings. Show bounded weekly/monthly priced totals and
  current/next fictional rate windows anchored to an explicit sample clock.
- Use ChargeShare as the page title and show separate Both cars/Joseph/Evan tab
  buttons under Charging costs, with an inverted selected state. Show session
  elapsed duration and start/stop times in collapsed rows. Replace the remaining native dropdowns with styled shadcn selects, add accessible rate/cost
  explanations and a small settings dialog for default vehicle and charging motion.
  Save only those display preferences in this browser.
- Add a subtle animated green border glow and faint overhead light rays to the
  active charging card, respecting the existing motion preferences.

The user explicitly requested implementation and localhost access. This preview
does not implement login, real battery readings, backend persistence, telemetry, monthly
invoices, payment, remote access or deployment.

## Capabilities

### New Capabilities

- `local-dashboard`: Browser review of synthetic charging and pricing states.

### Modified Capabilities

None. Core ledger and pricing contracts remain unchanged.

## Impact

Add `chargeshare-preview` to the Rust workspace for a synthetic read-only API,
using the local core, pinned tiny_http and serde_json. Add `apps/web` with React,
Vite, TypeScript and default shadcn primitives. This is the user's explicit
frontend exception to the earlier Rust-only app rule; core calculations remain
Rust with zero runtime dependencies. Node runs local development/build tools.
Retain both lockfiles. No cloud resource or real account is created.
