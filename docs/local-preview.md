# Local dashboard preview

The preview displays fictional Model Y battery widgets and sample charging
costs. It does not connect vehicles or calculate a household bill. Its Rust API
constructs synthetic events and calls the same core used by the acceptance tests.
The browser renders returned money strings without recalculating prices.

## Run

Use the Rust and Node prerequisites in the [README](../README.md#develop), then:

```sh
npm --prefix apps/web ci
npm run preview:dev
```

Open http://127.0.0.1:5173. The launch script builds the Rust preview, starts its
API at 127.0.0.1:8787, and starts Vite at 127.0.0.1:5173. Both ports must be free.
Ctrl-C stops both processes. No background login item or system service is installed.
If a repo-local toolchain exists in ignored `.tools/cargo` and `.tools/rustup`,
the launcher uses it; otherwise it uses cargo on PATH.

Frontend verification: `npm run preview:build` checks TypeScript and builds
`apps/web/dist`. This build does not deploy or embed an API. The local Vite proxy
is required for the documented preview; deployment needs a separate design.

## Interactions and expected results

| Scenario | Vehicle | Week cost / energy | Month cost / energy | Sessions needing review |
| --- | --- | --- | --- | --- |
| Complete | Both | $20.40 / 90 kWh | $26.00 / 118 kWh | 0 |
| Complete | Joseph | $11.60 / 52 kWh | $15.60 / 72 kWh | 0 |
| Complete | Evan | $8.80 / 38 kWh | $10.40 / 46 kWh | 0 |
| Missing reading | Both | $17.20 / 80 kWh | $22.80 / 108 kWh | 1 |
| Missing reading | Evan | $5.60 / 28 kWh | $7.20 / 36 kWh | 1 |

Use the three vehicle buttons under Charging costs to scope costs and sessions.
The selected button is white; arrow keys also change the view. Session duration
and start/stop times are visible in every row, for example `2hr (1:00am - 3:00am)`.
These are elapsed session times from the hourly fixtures, not active-charging
duration or live timestamps. Desktop shows aligned session, duration, energy and
cost columns; mobile keeps compact rows. Open the down caret or any part of a row
to inspect its rates or review reason. The caret points up while open.
Choose a sample scenario and press **Load scenario**
to rebuild it. Switching back restores the original fixture. The missing sample
belongs to Evan's 10 kWh session; its whole $3.20 quote is withheld, while the
observed 10 kWh remains visible. No partial cost is presented as complete.

The frozen sample clock is September 13, 2026 at 2 pm. The week covers September
7 through that clock; the month covers September 1 through that clock. Exact core
quotes are summed before each subtotal is rounded. Earlier-month sessions affect
only the month total; prior-month sessions affect neither total or the visible
list. This demo includes only closed sessions fully inside each period. It does
not implement real billing-cycle or cross-period allocation.

The two-rate example is 4 kWh at $0.20 plus 6 kWh at $0.40, producing $3.20.
These are fictional rates, not a utility tariff. The current rate is $0.20 until
3 pm, followed by $0.40 until midnight. Both the outlook and quotes use the same
synthetic schedule. Displayed dates map to synthetic hourly ticks in that schedule.
The demo does not resolve time zones, DST, seasons or real tariff dates.

Battery readings are static fixtures, not core telemetry. **Charging** is a demo
status and does not imply an active real charging session. No time-to-full,
real-time freshness or charging controls are claimed.

## UI and boundaries

React/Vite renders shadcn Base UI controls, native battery progress and native
session disclosures. The requested refinement uses near-black glass, regular
Geist weights and no theme switch or wordmark bar. Joseph's Model Y Quicksilver
and Evan's Model Y Black are explicitly requested display aliases over fictional
fixtures. Charging has a green fill, moving reflection, glow and pulsing indicator;
the active card also has a gently breathing border and three faint overhead rays.
The rays are deterministic CSS layers behind the content, inspired by the supplied
[Magic UI reference](https://magicui.design/docs/components/light-rays), with no
additional runtime dependency. They are pointer-transparent and decorative;
reduced transparency hides them.
The percentage remains a fixed sample. Reduced motion stops those animations and
retains the text status. Controls remain at least 40px high, and reduced
transparency has a CSS fallback. Assets and fonts are local.

Rate and cost help buttons show explanatory tooltips on hover, focus or tap.
Settings contains only a default vehicle view and charging animation preference.
Those preferences save automatically in versioned browser storage. Ordinary
vehicle filtering does not rewrite the saved default. Invalid or blocked storage
falls back to safe defaults or in-memory changes; failed saves are shown in Settings.
The operating system's reduced-motion preference always overrides animation.
These controls do not create a profile or authorize a Tesla connection.

The API accepts only documented GET scenario routes, returns safe errors for
unsupported inputs, and sets no-store/nosniff headers. It has no upload, write,
credential, OAuth, Tesla-control, storage or public-listener setting. The app
shows both fictional owners solely for demonstration; this is not production
authorization. Keep all real bills, account data and telemetry outside the repo.

No cloud project, deployment or paid service is required for this preview.

## Proposed hosting handoff

Keep the dashboard and pricing branch separate from the receiver work. Agree the
authenticated API contract and data ownership before integrating live telemetry.
Vercel could host this Vite frontend with root `apps/web`, build command
`npm run build` and output `dist`. The loopback Rust API and Vite development proxy
would not accompany that deployment; a hosted API or an explicitly static demo
would be a separate change.

The existing architecture proposes SQLite. Supabase Auth/Postgres is an optional
alternative to discuss with the backend maintainer, not a requirement for this UI.
The Tesla receiver remains a separately operated service. A Mac mini is a possible
host once its connectivity, authentication and operations are designed. This
branch creates no cloud projects or public endpoints.

For a future shared deployment, agree invite-only access before showing real
vehicle data. Google login is an optional convenience, and app sign-in is separate
from each owner's Tesla authorization. The local synthetic preview needs neither.
