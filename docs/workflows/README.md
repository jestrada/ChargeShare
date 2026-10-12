# Receiver workflow verification

The harness source was recovered from commit
`ec1c8922c813e8af57ea931324466ccc5afba5d5` and published on PR #5 without
rewriting history. The earlier environment could not publish workflow changes.
At PR #5 activation, the local GitHub login had the required workflow scope, so
the recovered workflow became active at
[telemetry-integration.yml](../../.github/workflows/telemetry-integration.yml).

Activation commit `4ea28f01445265b8e9e630cccc848895847a6d59` retained
`CHARGESHARE_CI_PROVE_FAILURE=1`. The [push run](https://github.com/jestrada/ChargeShare/actions/runs/38076460351)
and [PR run](https://github.com/jestrada/ChargeShare/actions/runs/38076463792)
both passed the authenticated receiver smoke, then failed the deliberately
missing-output multiset assertion. Both completed cleanup and uploaded only the
allowlisted summary and versions. The setting is now removed.
[Full acceptance](https://github.com/jestrada/ChargeShare/actions/runs/38080880179)
passed on commit `8455a45e937b7f9ad7359d4fcb9b1b46a111d847`.

Existing repository checks remain required. Activation does not prove startup,
transport, retention or lifecycle acceptance. Historical task status is preserved;
current acceptance and completion are tracked in GitHub Issues and PRs. Only allowlisted synthetic
summaries, versions and normalized fixture results are uploaded. Keys,
certificates and unrestricted runtime logs are excluded.

The workflow reuses pinned Nix-store and receiver build-layer caches.
Both are populated before generating test credentials. The receiver image is
loaded into the runner daemon and validated against this job's exact image ID,
Linux amd64, input hash and upstream revision. Tilt uses the validated image
without rebuilding; local development and teardown retain the reviewed Compose
source configuration. The cold receiver build/load/cache export took 4m22s;
the subsequent cache hit took 21 seconds. Nix restore plus repeated shell
verification took 33 seconds, versus 42 seconds for the cold shell verification
and another 15 seconds to create its cache. See [testing](../testing.md) for
runtime versions, complete scenario evidence and retained-result identity.
The first normal run reached all transport/negative cases but timed out after
the retained-state restart; allowlisted readiness diagnostics now expose resource
health, blockers and known failure categories instead of silently waiting.
Explicit Kafka protocol health and authenticated receiver health prerequisites
resolved the startup race. The passing run covered retained stop/start, two
identical clean reset/start results, invalid-trust shutdown and successful cleanup
in a 5m24s job. Reverify the exact published commit after final code or
documentation updates; the PR's hosted checks retain that final evidence.

## Durable ingestion gate for issue #27

The dedicated Ubuntu 24.04 ingestion workflow is present at
[ingestion-acceptance.yml](../../.github/workflows/ingestion-acceptance.yml).
It invokes the same bounded `bash scripts/dev/acceptance.sh all` used locally
through Tilt. The shared command now retains every ingestion phase and adds #21
viewing in a fresh epoch. No existing workflow file changed in #21.

Earlier #27 activation notes requested hosted deliberate failures. For the
current viewing acceptance Joseph's decision 4 allows local setup/assertion/timeout
runs followed by a restored local pass; a hosted deliberately failing viewing
run is not required. Each failure must reach its named stage and clean up owned
resources. Full receiver-backed acceptance and current hosted evidence are not
claimed by this sandbox. Only allowlisted summaries and versions are uploaded.
See [verification limits](../testing.md#issue-27-local-verification) and the
[current supervisor checklist](../persisted-viewing.md#supervisor-acceptance).

## Issue #21 clean viewing gate

The complete new workflow is inactive at
[ci-21-tmp/viewing-acceptance.yml](../../ci-21-tmp/viewing-acceptance.yml), with
[exact activation instructions](../../ci-21-tmp/README.md). Existing workflows
were not edited because the implementation token lacks workflow scope. The new
fresh Linux gate extends the shared acceptance through the owning runtime and
actual scoped HTTP, and verifies the dedicated persisted dashboard entry.
Joseph's decision 4 accepts local setup/assertion/timeout failure runs followed
by a restored local pass; no hosted deliberately failing viewing run is required.
The clean hosted gate and manual desktop/mobile evidence remain required.
[The supervisor checklist](../persisted-viewing.md#supervisor-acceptance) provides
exact commands, expected output, URLs and evidence/publication limits.
