# Persisted synthetic viewing

Issue [#21](https://github.com/jestrada/ChargeShare/issues/21) connects retained
synthetic evidence to an explicitly scoped browser view. The implementation has
local process/HTTP tests. Actual receiver-backed acceptance, the hosted viewing
gate and desktop/mobile browser evidence remain unverified in this sandbox.
Nothing here establishes physical accuracy, authentication or production durability.

## Runtime and response contract

`chargeshare-ingestion serve` opens SQLite once and owns ingestion and HTTP reads
for its entire lifetime. Its single owner loop polls the existing Kafka adapter,
commits through the existing WAL/FULL transaction, and serves scoped requests
between commits. A read replays one committed set into one core ledger; summary
and session details derive from that same ledger. Concurrent requests are served
serially. There is no per-request Store open, second writer or core dependency
on HTTP/SQL. Existing bounded ingestion and reread commands are retained for
acceptance while the long-lived owner is stopped; they refuse concurrent ownership.

The owner runs as the current user in the validated private Compose network,
using the pinned Kafka image as an execution environment, a read-only binary and
`/nix/store`, dropped capabilities and no writable container filesystem. Only the
owned private `.local-runtime/` is writable. Kafka still publishes no host port.
The owner binds `0.0.0.0:8788` **inside that private container**; the runner publishes
only IPv4 host `127.0.0.1:8788:8788`. Host/test invocations accept IPv4 loopback
addresses only. The all-interface bind is refused except for the exact reviewed
`/run/.local-runtime kafka:9092 0.0.0.0:8788` invocation. This container network
is a synthetic development boundary, not authentication.

`GET /api/poc/owners/{owner}/vehicles/{vehicle}/results` requires validated
1–64-character aliases and a configured owner/vehicle pair. Every returned
session validates `(vehicle, connection)` and owner. A malformed alias returns
400 `scope_refused`; unknown or mismatched pairs return 404 `scope_refused`.
There is no aggregate, mutation, pricing or fixture endpoint. Non-GET requests
return 405. Errors never echo rejected values. `/api/poc/ready` reports bounded
protocol/storage readiness separately from finite acceptance.

Responses contain only owner/vehicle aliases, six-decimal observed/eligible kWh
strings, session aliases, completeness, Unconfirmed classification, quality and
exclusion reasons, the synthetic physical-accuracy-unvalidated boundary,
allowlisted provenance and a semantic revision. No raw payload, device identity,
credentials, path, offset, receipt timestamp, database dump or log is returned.
Provenance identifies the synthetic source, source epoch, input contract, frozen
mapping/manifest, normalizer and `scoped-core-replay-v1` calculation.

Revision `results-v1-<16 hex digits>` is FNV-1a over the canonical ordered JSON
projection with its revision initially empty. It is a deterministic change
indicator, not a cryptographic integrity token. Requests, broker offsets,
redelivery and rejected raw text do not affect it. Scope, domain-visible energy,
quality, session completeness or visible provenance changes do. Reset replaces
the source epoch. Requests use `Cache-Control: no-store`.

The HTTP reader allows at most 4 KiB and a 250 ms request-read deadline, with a
250 ms socket-write timeout. Broker verification uses a shared two-second deadline
and repeats at least every two seconds of active polling. Storage binding/privacy,
source identity, retention and broker failures latch unavailable: readiness and
scoped reads return 503 `results_unavailable`, without current-looking cached
results. Restart the owning runtime after resolving the underlying failure.
SQLite recovery preserves atomic old-or-new commits. Native-client lifetime and
filesystem guarantees remain limited to the existing local recovery contract.

This small synchronous server avoids adding an async runtime or another HTTP
owner. Reusing a background HTTP framework plus a command queue is an alternative;
that would introduce extra scheduling and shutdown boundaries for this bounded
synthetic slice. The explicit parser deadlines cost a small amount of local
transport code, tested through actual HTTP. No domain policy or dependencies change.

## Browser contract

One `apps/web` package has two separate entry points and bundles. Vite's
`persisted` mode substitutes `persisted-main.tsx` **before** bundling and serves
5174 with only `/api/poc` proxied to 8788. Fixture mode remains 5173 → 8787 and
keeps its existing entry, fixture preferences and pricing behavior. The persisted
bundle imports no fixture data or fixture app. Both modes reuse the existing
Geist styling and shadcn Button; native session disclosures work by Enter/Space
or tap. Scope buttons work by Tab/Enter/Space and tap.

The persisted page labels the explicit owner/vehicle and Persisted/Synthetic
boundary. It renders exact returned energy strings, Unconfirmed, zero eligible
energy and hold reasons. Empty has no sessions or invented consumption summary.
Unavailable/refused replace prior results; failed requests never load fixtures.
A complete zero-energy session has `complete_zero` evidence and an explicit label;
it remains held for an Unconfirmed charger. Completeness and charger eligibility
are separate axes. Missing/conflicting evidence is `incomplete` with visible flags.
Quality and provenance remain reachable through the session disclosure at 375px.
No price, owed amount, battery, vehicle state, rate or calendar is displayed.

Requests time out after five seconds and repeat every ten seconds. A successful
last committed view can remain visible while its next read is in flight; failure
clears it. Scope changes hide the preceding scope immediately and abort the old
request. Refresh requests a new committed view. The page makes no collection
freshness or live vehicle claim.

## Supervisor acceptance

Run from the repository root on Linux x86_64 with permitted local Docker and Nix.
The commands below are required external verification, **not a recorded pass**.
Do not reuse another checkout's state. Normal shutdown preserves evidence. The
first reset is deliberate disposal of this checkout's stopped synthetic state.

```sh
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh down
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh reset --confirm-synthetic-reset
nix develop --no-update-lock-file -c bash scripts/dev/acceptance.sh all
nix develop --no-update-lock-file -c bash scripts/dev/acceptance.sh all --prove-failure setup
nix develop --no-update-lock-file -c bash scripts/dev/acceptance.sh all --prove-failure assertion
nix develop --no-update-lock-file -c bash scripts/dev/acceptance.sh all --prove-failure timeout
nix develop --no-update-lock-file -c bash scripts/dev/acceptance.sh all
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh up
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh check
```

Each normal acceptance must exit 0 with `Result: PASS`. Each deliberate proof
must exit nonzero with its exact `FAIL: setup`, `FAIL: assertion` or `FAIL: timeout`
and `Result: FAIL`, then remove owned resources/state. A prerequisite failure
cannot count as a deliberate-stage proof. Record the exact commit for the restored
pass. The shared command retains all #27 phases and then uses a fresh source epoch
for the long-lived owner: empty HTTP, fresh official receiver output, both scoped
HTTP results, repeated revisions, mismatch refusal, concurrent reads, actual
receiver duplicate/out-of-order traffic, committed progress and owner restart.
Its 40-minute outer bound and owned cleanup remain. Logs/captures stay private;
only `target/ingestion-acceptance-results/summary.md` and allowlisted versions are
publication evidence. #21's frontend tests and builds are added to the existing
ingestion acceptance workflow, staged at
[ci-21-tmp/ingestion-acceptance.yml](../ci-21-tmp/ingestion-acceptance.yml)
until Joseph swaps it in.

`poc.sh` serializes lifecycle commands with a private controller lock; a competing
command refuses without signaling resources. Startup interruption follows owned
teardown and preserves state.

`up` returns after bounded all-service readiness and leaves the stack running;
it sends no fixtures. Expected output names persisted 5174/8788 and fixture
5173/8787, receiver and private Kafka. `check` sends a fresh finite complete
receiver run, checks ACK plus matching Kafka, waits for its exact six accepted
SQLite deliveries/progress, asserts both actual HTTP results and checks the
persisted browser entry. It prints two `PASS:` lines naming the path and scoped
values. Readiness or stale evidence alone cannot pass. `check` preserves state
and can be repeated. Full recovery/refusal acceptance is `acceptance.sh all`.

Compare these actual network responses with the browser:

```sh
curl --fail --max-time 10 --silent --show-error http://127.0.0.1:8788/api/poc/owners/owner-a/vehicles/vehicle-a/results
curl --fail --max-time 10 --silent --show-error http://127.0.0.1:8788/api/poc/owners/owner-b/vehicles/vehicle-b/results
curl --max-time 10 --silent --show-error --write-out '\nHTTP %{http_code}\n' http://127.0.0.1:8788/api/poc/owners/owner-a/vehicles/vehicle-b/results
```

Expect A `0.500000` and B `0.250000` observed AC kWh, `connection-1`, Unconfirmed,
`0.000000` eligible, no quality flags and `UnconfirmedCharger` exclusion. Mismatch
must return only `{"error":"scope_refused"}` with HTTP 404. Repeat both reads and
compare revisions. Record the revisions before and after:

```sh
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh restart
```

Expect preserved epoch, session identity, energy, holds and revision, with no
double counting. Ordinary restart does not reset. Leave `up` running for Joseph's
review. The URLs are:

- A: http://127.0.0.1:5174/?owner=owner-a&vehicle=vehicle-a
- B: http://127.0.0.1:5174/?owner=owner-b&vehicle=vehicle-b
- Independent fixtures: http://127.0.0.1:5173
- Tilt resource view: http://127.0.0.1:10350

At **1280×1000** and **375×812**, capture both scopes; inspect each actual
`/api/poc/.../results` response in the browser Network panel and compare aliases,
energy, completeness, holds and revision with the page. At desktop use Tab and
Enter/Space to select scopes and expand `connection-1`. At mobile tap the scope
and disclosure. Capture expanded details at both sizes. Verify no horizontal
scroll at 375px and no price/owed/battery/status/rate/calendar fields. Use only
allowlisted fictional screenshots and curated summaries, never HAR files, raw
logs/payloads, trust files or database exports. Do not mark browser acceptance
complete based on focused parser tests or build success.

## Empty and unavailable reproduction

Empty is easy to produce without writing directly to SQLite: stop, explicitly
reset this synthetic dataset, then start without `check`:

```sh
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh down
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh reset --confirm-synthetic-reset
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh up
```

Open both persisted scope URLs. API must report `state: "empty"`, `sessions: []`;
the browser says **No committed sessions**. Capture at both widths. Startup
creates an owned empty store, not preloaded results. Afterwards `poc.sh check`
produces the held 0.5/0.25 kWh results again.

To stop only the API owner while leaving the 5174 browser server running:

```sh
nix develop --no-update-lock-file -c python3 scripts/dev/viewing.py stop-owner
```

Click **Refresh results**. The page must say **Persisted API unavailable** and
remove previous sessions/energy; it never loads fixtures. Capture at both widths.
A refused scope can also be inspected at
http://127.0.0.1:5174/?owner=owner-a&vehicle=vehicle-b. Restore with:

```sh
nix develop --no-update-lock-file -c tilt trigger persisted-runtime
```

Refresh when readiness returns. If recovery needs a full restart, use
`poc.sh restart`. `tilt trigger` is not a reset.

Final shutdown and reset remain separate:

```sh
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh down
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh reset
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh reset --confirm-synthetic-reset
```

`down` stops only owned resources and preserves source epoch and SQLite. An
unconfirmed reset prints exact scope and fails without deletion. Confirmed reset
requires stopped owned containers, no managed listeners, no tracked Tilt/harness
and the exclusive synthetic run lock; active or unowned state refuses. It removes
only this checkout's synthetic runtime and owned broker volume, preserving source
and browser preferences. It never prunes Docker. Next `up` creates a fresh epoch.
