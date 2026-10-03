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
| Complete | Both | $27.38 / 90 kWh | $35.14 / 118 kWh | 0 |
| Complete | Joseph | $15.63 / 52 kWh | $21.18 / 72 kWh | 0 |
| Complete | Evan | $11.75 / 38 kWh | $13.97 / 46 kWh | 0 |
| Missing reading | Both | $23.40 / 80 kWh | $31.16 / 108 kWh | 1 |
| Missing reading | Evan | $7.77 / 28 kWh | $9.99 / 36 kWh | 1 |

The top card shows Joseph’s eligible sample month subtotal payable to Evan
($21.18 in September; $21.02 in October), independent of the session filter.
It is a sample calculation, not an invoice or payment status. Holds stay excluded
and counted; if no consumption can be priced, it says Needs review.

A fresh browser defaults to Joseph; existing saved view preferences are respected.
Use the three vehicle buttons under Charging costs to scope costs and sessions.
The selected button is white; arrow keys also change the view. Session duration
and start/stop times are visible in every row, for example `2hr (1:00am - 3:00am)`.
These are elapsed session times from the hourly fixtures, not active-charging
duration or live timestamps. Desktop shows aligned session, duration, energy and
cost columns; mobile keeps compact rows. Open the down caret or any part of a row
to inspect its rates or review reason. The caret points up while open.
In **Settings**, choose a sample scenario and press **Load scenario**
to rebuild it. Switching back restores the original fixture. The missing sample
belongs to Evan's 10 kWh session; its whole $3.98 quote is withheld, while the
observed 10 kWh remains visible. No partial cost is presented as complete.

The frozen sample clock is September 13, 2026 at 2 pm. The week covers September
7 through that clock; the month covers September 1 through that clock. Exact core
quotes are summed before each subtotal is rounded. Earlier-month sessions affect
only the month total; prior-month sessions affect neither total or the visible
list. This demo includes only closed sessions fully inside each period. It does
not implement real billing-cycle or cross-period allocation.

## Dated public sample rates

The default uses the September historical public EV2-A sample, replacing the
fictional 0.20/0.40 prices. Settings also offers an October winter-estimate sample
with the same synthetic session shape and an October 13, 2026 sample clock.

| Version | Effective from (inclusive) | Until (exclusive) | Off-peak | Partial-peak | Peak |
| --- | --- | --- | --- | --- | --- |
| `ev2a-summer-2026` | 2026-09-01 | 2026-10-01 | 0.2773931020 | 0.4783693788 | 0.5866379696 |
| `ev2a-winter-2026-est` | 2026-10-01 | 2027-06-01 | 0.2779120234 | 0.4465438926 | 0.4624755018 |

All amounts are USD/kWh decimal strings. Daily windows are off-peak 00:00–15:00,
partial-peak 15:00–16:00 and 21:00–24:00, and peak 16:00–21:00. The winter end
bounds the estimate to the stated October–May season; it is not a guarantee that
rates stay unchanged throughout that season. The UI labels it **Winter estimate,
unverified**. These user-supplied public sample rates are not derived from private
bill contents and are not represented as the current tariff for any account.

Each expanded priced session identifies its rate version. Inline period labels
show measured splits: September 11 has 4 kWh off-peak plus 6 kWh partial-peak,
with exact cost 3.9797886808 USD ($3.98 displayed). A missing boundary reading
shows both periods but no made-up energy split. Rate displays round to four
places ($0.2774); the unchanged core uses all ten decimal places. Totals sum
exact quotes before rounding once, not the rounded displayed rates or line costs.

Both the outlook and quotes use the same schedule. Missing coverage stays held;
the outlook displays Rate unavailable and never skips a gap to borrow a later
price. The fixture associates its month/day with synthetic hourly ticks explicitly.
It does not implement UTC, time zones, DST or Tesla timestamp conversion.

Battery readings, “last updated” values and the charging car’s 6 kWh added are
static labeled fixtures, not telemetry. The active fixture is not a billable
closed session and contributes nothing to reimbursement. Refreshing the page does
not change its sample update time or imply fresh data. Vehicle cards are compact
on mobile and follow the reimbursement card.

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
Settings contains sample scenarios, a default vehicle view and charging animation preference.
Only the display preferences save automatically in versioned browser storage. Ordinary
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
