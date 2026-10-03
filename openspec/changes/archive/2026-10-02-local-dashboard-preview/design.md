# Design

## Context

The offline pricing change is complete locally. See [proposal](proposal.md) for
the browser requirement. The domain core remains independent of transport and UI.

## Goals / Non-Goals

Render the real synthetic ledger results in a local visual prototype. Keep
battery fixture presentation separate from the core, which does not ingest SOC.
This is not the production authenticated website or a receiver integration.

## Decisions

- Add a separate `chargeshare-preview` Rust crate with demo/API responsibilities
  and a small main. Dependencies point from preview to core. React rendering
  lives in `apps/web` and never recalculates prices with JavaScript numbers.
- Use pinned tiny_http 0.12.0 to avoid writing an HTTP parser. It is isolated to
  the preview and lockfile; no TLS, cloud service or core runtime dependency.
- Bind the API to 127.0.0.1:8787 and Vite to 127.0.0.1:5173 with strict ports.
  Proxy the GET-only API through Vite. Reject unknown query names/values and duplicate keys;
  never interpolate untrusted query text. Add no file serving or upload route.
- Use default shadcn buttons, styled Base UI selects and table with native details controls.
  Fetch one scenario snapshot with cancellation; derive the vehicle view without
  redundant requests. Preserve money strings from the Rust API.
- Apply the latest refinement: near-black glass, locally bundled Geist at regular
  weight, no wordmark/top bar or theme option. Green charging glow and restrained
  motion communicate activity alongside text; reduced motion disables animation.
  Battery values remain fixed samples. First-name/color display aliases were
  explicitly requested; vehicle/account identifiers and readings remain fictional.
- Add earlier-month and prior-month fixtures. Sum exact core quotes separately for
  the sample Monday-Sunday week and month to date before rounding. Exclude held
  sessions and out-of-period records; show held counts and explicit date bounds.
  This is preview aggregation, not a production billing/calendar API.
- Use one fictional daily schedule for session prices and a current/next-window
  display. Freeze the demo clock at September 13, 2026, 14:00 and label it. The
  window lookup uses half-open boundaries; no real EV2A rate or live clock is claimed.
- Seed both owners with complete confirmed sessions. One known synthetic boundary
  reading is omitted only in the missing scenario. Prices and review states are
  calculated anew per request. Filter summaries before rendering aggregate cards.
- Supply no-store and nosniff API response headers. Vite remains a local dev
  server. No listener configuration allowing LAN/public binding is exposed.
- Use a plain ChargeShare page title. Three visible shadcn tabs under Charging
  costs scope both totals and sessions, keeping the selected vehicle one tap away.
  Tabs appear as three separate buttons, with the active choice white on black.
  They retain Base UI keyboard/focus semantics and labeled panels. Session rows
  expose the API's start/stop time label with elapsed duration, such as
  `2hr (1:00am - 3:00am)`, without opening the disclosure. The preview formats
  these labels from its existing hourly session boundaries, not parsed display text;
  desktop rows align session, duration, energy and cost in four labeled columns.
  Narrower layouts place energy below cost to leave room for readable times.
  A down caret rotates upward when the native disclosure opens.
- Replace the remaining browser-native select popups with shadcn Select for consistent dark
  surfaces, keyboard selection and portaled placement. Keep the existing Base UI
  dependency; add its standard Tabs, Tooltip, Dialog and Switch wrappers rather than
  custom focus/positioning code. Tooltips supplement visible copy, open on focus
  or hover and support tapping their labeled help buttons.
- Keep settings limited to default vehicle view and charging animation. A versioned
  localStorage record contains only an allowlisted fixture vehicle choice and a
  boolean; invalid or unavailable storage falls back safely. Preferences apply
  immediately and are separate from account identity. OS reduced motion always
  wins. A separate profile or Google login would require the future shared access
  design and is not implemented by this refinement.
- The UI owns these preferences and explanatory copy. The Rust preview only
  refines its existing time label; pricing and core contracts are unchanged.
  A server preferences API would add account/storage dependencies without
  helping this local preview. Verify selects, focus return, Escape, touch targets,
  tooltip access, reload persistence and storage failures in the browser.
- Refine the active charging card with an opacity-animated inset border halo and
  three soft overhead rays behind its content. Use deterministic CSS gradients,
  transforms and opacity, inspired by the supplied Magic UI light-rays reference.
  The effect needs no random state, animation dependency or data/API change.
  Decorative layers are pointer-transparent and hidden from assistive technology.
  The saved animation preference and OS reduced motion stop every new animation;
  reduced transparency hides the rays. Idle vehicles never render them.

## Risks / Trade-offs

- Sample battery values could look live → label both the page and readings as demo,
  and display that vehicles are not connected.
- Local preview could be mistaken for production → document absent auth/storage,
  hardcode loopback and expose no credential or connection form.
- Synthetic scenario totals could drift from the engine → route tests check the
  known complete/missing results and scoped filtering; visual verification covers
  layout, interactions, console errors and narrow-screen overflow.
- tiny_http is a new dependency → pin it, retain Cargo.lock, and use its official
  documented server/request/response API with default TLS features disabled.

## Verification

Run workspace tests, formatting and Clippy, strict specs and security scans.
Use agent-browser to verify desktop/mobile views, scenario changes, vehicle
filters and session details, then leave the local server available for review.
