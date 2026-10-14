# ChargeShare

A Rust charging ledger for sharing a home charger. The local proof of concept
sends fictional telemetry through the official receiver and Kafka into SQLite,
then shows scoped sessions in a React/shadcn dashboard. An independent fixture
preview demonstrates exact offline pricing with labeled sample rates. Everything
is synthetic; no live vehicle or account is connected.

## Quickstart

Use Linux x86_64 or aarch64 with **Nix** (`nix-command` and `flakes` enabled) and a running
local **Docker** daemon your user can access. Nix supplies the pinned development
tools; it does not install or start Docker. Remote Docker contexts are refused.
Run from the repository root; first startup can take several minutes.

```sh
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh up
```

Open **http://127.0.0.1:5174**. Startup sends no fixtures, so a fresh dataset is
empty. Populate and check the receiver → Kafka → SQLite → HTTP path:

```sh
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh check
```

Choose `owner-a` / `vehicle-a` or `owner-b` / `vehicle-b` in the dashboard. The
complete sample has 0.500000 / 0.250000 observed AC kWh; sessions currently stay
`Unconfirmed` with zero eligible energy. Automatic location classification is
planned in [#22](https://github.com/jestrada/ChargeShare/issues/22).

Restart or stop while preserving evidence:

```sh
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh restart
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh down
```

After stopping, explicitly discard only this checkout's synthetic runtime and
broker volume (the next `up` creates an empty dataset):

```sh
nix develop --no-update-lock-file -c bash scripts/dev/poc.sh reset --confirm-synthetic-reset
```

Without the confirmation flag, `reset` lists its scope and refuses deletion.
The independent fixture preview is at http://127.0.0.1:5173 while the stack runs.

## Tests

Enter the pinned shell, then run the applicable checks:

```sh
nix develop --no-update-lock-file
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/testing/offline-suite.sh
bash scripts/testing/ingestion-suite.sh
python3 scripts/testing/test_ingestion_guard.py
python3 -m unittest discover -s scripts/dev -p 'test_*.py'
npm --prefix apps/web ci
npm --prefix apps/web test
npm run preview:build
npm --prefix apps/web run build:persisted
node scripts/security/check-paths.mjs
```

Both Rust suite wrappers run `cargo test --workspace --locked` and guard against
missing or filtered acceptance suites. For full receiver/recovery/viewing
acceptance, use a fresh, stopped runtime with no retained state:

```sh
nix develop --no-update-lock-file -c bash scripts/dev/acceptance.sh all
```

CI runs [repository checks](.github/workflows/security.yml),
[receiver transport](.github/workflows/telemetry-integration.yml) and
[ingestion/viewing acceptance](.github/workflows/ingestion-acceptance.yml).
Read [SECURITY.md](SECURITY.md) for scanner/hook setup and publication checks;
record run results in the owning GitHub issue or PR.

## Documentation

- [Architecture](docs/architecture.md): components, data flow, ports, domain contracts and decisions.
- [Code design](docs/code-design.md): design-phase guidance for cohesive Rust modules and inward dependencies.
- [Workflow](docs/workflow.md): how each feature goes from issue to merged PR.

Contributors follow [AGENTS.md](AGENTS.md). Original code and documentation are
[MIT licensed](LICENSE); [upstream notices](docs/licenses/) remain applicable.
