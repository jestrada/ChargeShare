# Local synthetic receiver environment

This stage adds an official Tesla receiver → Kafka transport harness alongside
ChargeShare's existing fixture API/dashboard. The receiver does not feed the
Rust ledger, SQLite or dashboard. A passing test requires the exact expected
receiver acknowledgment **and** matching decoded Kafka records. It makes no
utility-meter, exactly-once or production-durability claim.

## Supported entry and prerequisites

Initial target: Linux x86_64, including a suitable private cloud Linux machine.
Other operating systems and architectures are deferred. Install Nix 2.28.6 with
`nix-command` and `flakes` enabled, plus a permitted, running host Docker daemon
for Linux x86_64 containers. Your user must already have daemon access. Nix
supplies tools; it does not install/start Docker, change socket permissions or
provision cloud infrastructure. Remote Docker contexts are rejected.

Use the repository root:

```sh
nix develop --no-update-lock-file
bash scripts/dev/versions.sh
tilt up
```

The shell pins Rust 1.99.0/rustfmt/Clippy, Tilt 0.37.7, Docker CLI 29.8.1,
Compose 5.5.1, Node 24.21.0/npm, Go 1.27.1 and utilities. `flake.lock`,
`Cargo.lock` and both npm lockfiles remain authoritative. First entry downloads
Nix store outputs; first start downloads immutable container inputs, compiles
native upstream dependencies and Rust, and runs the locked frontend install.
These cold steps can take several minutes and need network access. Neither
entry nor startup upgrades locks. The shell's Tilt launcher preflights daemon,
platform, all required ports and exact synthetic configuration before starting
Tilt, then binds its UI to 127.0.0.1:10350. Lifecycle use outside the repository
root is refused. Direct unwrapped Tilt lifecycle commands are unsupported.

The official receiver is unpatched at
`bd076fe1494841707528449560c4a19d0d426da4`. The default upstream production CA
bundle remains trusted, alongside the generated test CA; upstream can select
its engineering bundle instead of production. The synthetic client trusts only
the generated server CA. This explicit choice does not enroll real vehicles or
authorize live traffic. The exact config has no Tesla account credentials,
vehicle configuration or outbound Tesla API calls. Unrelated generated CAs
remain untrusted.

## Resources and endpoints

| Tilt resource | Responsibility / readiness |
| --- | --- |
| kafka | Single KRaft broker, private container `kafka:9092`; Kafka metadata request |
| receiver | Official decoded JSON dispatcher; authenticated `/status` on loopback `https://127.0.0.1:8443` |
| rust-build | Locked workspace compilation, including outer transport harness |
| frontend-install | Locked `apps/web` npm install |
| fixture-api | Existing Rust fixture API at `http://127.0.0.1:8787/api/demo` |
| fixture-dashboard | Existing React/shadcn fixture demo at `http://127.0.0.1:5173` |
| receiver-ready | Kafka metadata/topic plus authenticated receiver status |
| telemetry-smoke | Manual, finite ACK-plus-Kafka assertion |

Tilt UI: `http://127.0.0.1:10350`. The demo and synthetic-transport labels are
separate. No Kafka host port, profiler port, public tunnel or ingress is exposed. Kafka's image-declared secrets/config paths use private tmpfs mounts, so they do not leave anonymous volumes after shutdown.
A Compose bridge network isolates internal endpoints; it is not a production
security boundary. Runtime generation has directories 0700/files 0600 under
ignored `.local-runtime/`; keys are read-only mounts and never image inputs,
Git content, Nix inputs, system trust or public artifacts.

```sh
tilt logs kafka receiver
tilt logs fixture-api fixture-dashboard
tilt trigger telemetry-smoke
tilt wait --for=condition=Ready uiresource/telemetry-smoke --timeout 90s
bash scripts/dev/harness.sh all
bash scripts/dev/harness.sh invalid-auth
bash scripts/dev/harness.sh replay
```

All scenarios are finite and fictional. See [fixture provenance and exact
semantics](telemetry-fixtures.md). Each run holds an exclusive process lock and
records the end offset of every configured partition before sending; the local
fixture topic intentionally has one partition. Old records cannot satisfy a
new run. JSON payload/key/source-time/value comparison retains missing fields
and duplicate records, with ordering normalized across vehicles. Replay starts
from the saved independent position in `.local-runtime/last-run.json` and
checks local broker retention only.

## Deadlines and failures

Initial protocol readiness has a 15-minute cold-start bound in CI. Each Docker
verification command has a 25-second deadline; Kafka record observation is
20 seconds. TLS/WebSocket connect, reads and writes have 10-second bounds.
The upstream producer has `acks=all` and a 10-second message timeout, with
reliable V acknowledgments configured to Kafka. This single-broker development
policy is separate from future database/offset atomicity and durability.

A stage-specific nonzero exit identifies prerequisite, authentication,
acknowledgment, broker-observation or matching failure. An ACK alone cannot
pass. Negative checks use the same run-start isolation:

```sh
bash scripts/dev/harness.sh smoke --no-send
bash scripts/dev/harness.sh smoke --inject-missing-output
```

Both must fail, including when earlier valid fixture records are retained.
For an unavailable daemon, start a permitted host daemon and retry. For an
occupied port, stop the application using it. For unsafe config, restore the
reviewed `dev/compose.yaml` and `dev/receiver.synthetic.json`. No helper alters
host settings or weakens TLS verification to recover a failure.

## Shutdown, retention and explicit reset

```sh
tilt down
```

The shell launcher verifies and signals only this checkout's recorded Tilt
process, waits for host preview listeners to close, then removes this project's
Compose containers/network. Broker volume and local runtime state remain. Shutdown does not prepare or
validate certificate contents, so expired or incomplete test trust cannot block
teardown; reviewed configuration and runtime ownership checks still apply.
Closing the browser UI does not stop anything; Ctrl-C stops Tilt and its host
processes but leaves Compose containers. Run `tilt down` for full shutdown.
Restarting with `tilt up` retains broker data and regenerated verifier processes
can run the saved replay check.

Destructive reset is separate:

```sh
python3 scripts/dev/runtime.py reset
python3 scripts/dev/runtime.py reset --confirm-synthetic-reset
tilt up
```

The first command lists exact scope and exits without deletion. The confirmed
command refuses active project containers or occupied managed listeners, checks
ownership labels and removes only this stopped project's broker volume,
containers and `.local-runtime/`. The empty private `.local-runtime-run.lock` coordination file is retained, so a concurrent run cannot replace the locked inode during reset. It never runs Docker prune or touches another
checkout, source files or browser state. Certificates regenerate on the next
start. Two confirmed reset/start/scenario paths must have identical normalized
semantic results; volatile receipts/offsets are not energy evidence.

## Automated acceptance and evidence

[The integration workflow](../.github/workflows/telemetry-integration.yml)
is active for every push and pull request. [Verification steps](workflows/README.md)
describe the deliberate failure proof and restored successful run. It runs using a fresh `ubuntu-24.04` x86_64 VM, its
host Docker daemon and the same locked shell/Compose/Tilt inputs. It explicitly
triggers `telemetry-smoke`, checks all fixture cases and negative boundaries,
replays retained records across verifier and service restarts, and compares two
full clean resets. An overall timeout and exit-preserving cleanup bound the job.
Only versions, stage summaries and synthetic normalized results are uploaded;
private material and unrestricted local logs are excluded. Cloudflare,
ingestion, SQLite and receiver-backed dashboard behavior are not covered.

Implementation validation on this editing sandbox: flake evaluation/lock
stability, pinned CLI version checks, Rust formatting/Clippy/workspace tests,
protocol fixture regeneration and runtime guard tests are available. A host
Nix store and Docker daemon are absent here. Full `nix develop`, image builds,
Tilt orchestration and the actual receiver/Kafka path require the exact-commit
hosted result before they are called verified. See [testing](testing.md) for
accepted evidence, updated only after the hosted job reaches a terminal result.
