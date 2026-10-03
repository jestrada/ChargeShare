# ChargeShare

A private charging ledger for sharing a home charger, built in Rust.

## Start here

Spec 1 implements an offline, in-memory synthetic multi-vehicle ledger with
separate observed AC energy and conservative shared-charger eligibility,
deterministic replay and explicit uncertainty. Nothing is connected to Tesla.
Owner/vehicle scope checks are not authentication or approved cross-owner sharing;
physical measurement accuracy is unvalidated. Prices, bills, statements, live
collection, receiver integration, persistence, UI and deployment are outside scope.

The three guides in `docs/` are:

- [Architecture and domain contract](docs/architecture.md): implemented behavior,
  system diagram, measurement limits and proposed integration boundaries
- [Code design](docs/code-design.md): self-documenting, agent-friendly code,
  cohesive modules, inward dependencies and the decision/review workflow
- [Testing](docs/testing.md): commands, scenario coverage, verification evidence
  and later receiver/real-car validation gates

[Spec 1](openspec/changes/archive/2026-10-03-offline-multi-vehicle-ledger/proposal.md)
was implemented, verified and archived on 2026-10-03. Its seven accepted
requirements are the [vehicle-ledger contract](openspec/specs/vehicle-ledger/spec.md).
No changes are currently active. The archived planning artifacts retain their
original review context. The earlier broad single-vehicle change was deleted,
not marked complete.

## Develop

The Cargo workspace starts at `crates/chargeshare-core/`. There is no executable
application or Tesla integration to launch. Rust is the application language;
Node.js/npm is development tooling only.

Prerequisites: Rust 1.99.0 through rustup with rustfmt/Clippy, a C linker, Node.js
24 or newer, npm, Git, Bash, curl, tar and SHA-256 tooling. The toolchain is pinned
in `rust-toolchain.toml`; `Cargo.lock` is committed and the core has no runtime
dependencies. OpenSpec 1.14.0 is pinned with registry integrity hashes.

```sh
npm ci --ignore-scripts
bash scripts/security/install-gitleaks.sh
bash scripts/security/install-hooks.sh
npm run spec:validate
npm run spec:status
node scripts/security/test-guards.mjs
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/testing/offline-suite.sh
```

The suite runs complete `cargo test --workspace --locked` and retains synthetic
output under ignored `target/spec1-test-results/`. See [testing](docs/testing.md)
for coverage and CI evidence. Read [SECURITY.md](SECURITY.md) before committing;
a fresh clone does not activate hooks automatically. Gitleaks 8.30.1 is downloaded
from its official release and checksum-verified by the installer.

The npm package's `private: true` prevents npm publication, not GitHub visibility.
The npm OpenSpec scripts opt out of telemetry; use `OPENSPEC_TELEMETRY=0` or
`DO_NOT_TRACK=1` for direct CLI commands. `.env.example` contains inert placeholders;
copying it neither creates credentials nor authorizes live operation.

## Change and publication workflow

1. Read [AGENTS.md](AGENTS.md), the three guides, the
   [main contract](openspec/specs/vehicle-ledger/spec.md) and archived Spec 1's
   [proposal](openspec/changes/archive/2026-10-03-offline-multi-vehicle-ledger/proposal.md),
   [design](openspec/changes/archive/2026-10-03-offline-multi-vehicle-ledger/design.md),
   [scenarios](openspec/changes/archive/2026-10-03-offline-multi-vehicle-ledger/specs/vehicle-ledger/spec.md)
   and [tasks](openspec/changes/archive/2026-10-03-offline-multi-vehicle-ledger/tasks.md).
2. Use the generated OpenSpec skills under `.agents/skills/` to propose/refine
   requirements and scenarios. Obtain explicit implementation approval for new
   behavior; later capabilities need separately reviewed specs.
3. Run all applicable tests, strict spec validation and [publication safeguards](SECURITY.md#before-every-publication).
   Stage only intended files and review the complete staged diff and commit author
   metadata. Stop on scanner findings or unexpected files; never bypass hooks.
4. Publish only to the authorized repository/branch with a normal fast-forward
   push. If the remote changed, integrate deliberately and repeat checks; never
   force-push or replace Git history as a shortcut.
5. Verify the exact remote commit and terminal hosted checks before claiming
   completion. Archive only after implementation and verification. Passing checks
   do not authorize merging, deployment, Tesla registration, OAuth/key pairing,
   credential creation, vehicle access or spending.

## Licensing and tooling provenance

No project license has been selected. Public visibility alone grants no general
open-source license. OpenSpec is a development dependency. The official
Fission-AI/OpenSpec 1.14.0 CLI generated `.agents/skills/openspec-*` and
`.agents/skills/.openspec-target`; their MIT metadata and the full
[upstream MIT notice](docs/licenses/openspec-MIT.txt) are retained. The notice is
legal attribution, not a fourth guide.

The initial generation command was
`OPENSPEC_TELEMETRY=0 npx @fission-ai/openspec@1.14.0 init --tools codex --profile core --no-animation`.
This is provenance, not a command to rerun on each checkout. Gitleaks is MIT-licensed;
its executable is not committed. Its pinned version, official source and digests
are in [the installer](scripts/security/install-gitleaks.sh). No private project
code, personal correspondence, live vehicle payloads or proprietary integration
implementation was copied into this repository.
