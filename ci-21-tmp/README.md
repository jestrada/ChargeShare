# Issue #21 viewing gate activation

The implementation token has no workflow scope. No file under
`.github/workflows/` was changed. This complete workflow is inactive until Joseph
moves `ci-21-tmp/viewing-acceptance.yml` to
`.github/workflows/viewing-acceptance.yml` in a separately authorized change with
a token permitted to publish workflows. Keep the existing ingestion and transport
gates. Publish through `gh stack` and verify the exact remote head and terminal
hosted viewing check before recording acceptance in issue #21.

```sh
mv ci-21-tmp/viewing-acceptance.yml .github/workflows/viewing-acceptance.yml
```

The clean Ubuntu 24.04 gate uses pinned Nix, receiver build inputs and the shared
`acceptance.sh all`, now including actual scoped HTTP and the persisted dashboard
entry. It also runs focused guards, all Rust tests, Python helpers, frontend tests
and both builds. The existing repository workflow retains publication scans.
Artifacts contain only allowlisted versions and synthetic summaries. Runtime
trust, decoded payloads, databases, unrestricted logs and caches are excluded.

Joseph's decision 4 allows local setup/assertion/timeout proof runs followed by a
restored passing local run. No hosted deliberately failing run is required.
[The exact supervisor checklist](../docs/persisted-viewing.md#supervisor-acceptance)
contains those commands and the manual browser evidence requirements. This file
and successful focused tests establish no receiver-backed or hosted pass.
