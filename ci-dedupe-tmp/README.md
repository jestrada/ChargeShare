# Staged CI workflow replacements

These files are the intended replacements for `.github/workflows/security.yml` and `.github/workflows/telemetry-integration.yml`. Before merging, a maintainer with workflow permission must move them into `.github/workflows/`, overwriting the originals, and delete `ci-dedupe-tmp/` in the same PR.

The replacements run on every pull request and every push to `main`; Repository checks also supports manual dispatch. Concurrency allows one active run per PR and cancels superseded PR runs, while main runs are never cancelled. Branches without an open PR get no hosted checks.

[docs/testing.md](../docs/testing.md) and [docs/local-development.md](../docs/local-development.md) already describe this post-move behavior.
