# Security policy

ChargeShare is an early-stage public repository with an offline synthetic Rust
ledger, not a production service.
No version is currently suitable for real credentials, vehicle access, or personal
data. Only synthetic examples belong in this repository.

## Reporting a vulnerability

Do not publish credentials, personal data, or exploitable details in an issue or
pull request. Use GitHub's private **Report a vulnerability** option on this
repository's Security tab if it is available. If no private channel is available,
open a minimal issue asking the maintainer to establish one, without sensitive
details. Do not assume private reporting is enabled.

## If a secret is exposed

1. Revoke or rotate the credential immediately at its issuer.
2. Tell the maintainer through a private channel. Do not paste the secret again.
3. Assess use and scope, remove the exposure, and coordinate any history cleanup.
   Removing a file or rewriting Git history does not revoke a secret or erase
   copies, caches, or forks.
4. Re-run the staged and complete-history scans before publishing again.

## Data boundary

This repository must contain only public-safe source, plans, and synthetic test
fixtures. Never copy secrets or source from private, workplace, vehicle-provider,
or existing application repositories into it. Do not commit real VINs, location
history, charging-session exports, account identifiers, invoices, access tokens,
private keys, or screenshots/logs containing these values. Use `.invalid` domains
and clearly fictional identifiers in examples.

`.env.example` is the only allowed environment template. Keep it to empty values
or unmistakably inert placeholders. `.gitignore` is a convenience, not a security
boundary: ignored files can still be force-added, and previously tracked files
stay tracked. The sensitive-path check separately inspects Git's index.

## Local safeguards

Follow the [development setup](README.md#develop). The installer downloads
Gitleaks 8.30.1 from its official GitHub release, verifies a SHA-256 digest pinned
in the script, and installs it only into ignored `.tools/bin/`. It supports Linux
and macOS on x64 or arm64. Review upstream release notes and refresh version and
digests together when upgrading. On other platforms, use an official 8.30.1 binary
on PATH or run the checks in Linux/WSL.

The hooks installer sets this checkout's `core.hooksPath` to `.githooks` and
refuses to overwrite a different configured hooks path. Cloning does not install
hooks. Pre-commit checks staged content; pre-push checks all available local Git
history. Both fail closed if the scanner or required runtime is missing.

## Before every publication

```sh
git diff --cached --check
bash scripts/security/scan.sh staged
bash scripts/security/scan.sh history
```

Run `node scripts/security/test-guards.mjs` to exercise the guardrails against
synthetic fixtures in a temporary repository.

Inspect `git diff --cached` manually as well. Verify `.env.example` contains only
placeholders and every fixture is fictional. Review commit author metadata before
publishing; use a verified GitHub noreply address when personal email privacy
matters. Do not copy credentials into commands or command output.

Gitleaks extends its built-in rules without blanket allowlists. Scan output uses
100% secret redaction; inline allow comments and `.gitleaksignore` are ignored by
the wrapper. Do not create baselines that hide an unresolved exposure. Review
false positives carefully rather than disabling a rule or bypassing hooks.

## CI and limits

GitHub Actions scans tracked paths and complete fetched history and checks the
Rust workspace, frontend build and synthetic receiver integration. OpenSpec
validation is paused with its workflow. Actions are pinned to immutable commit
IDs; Gitleaks downloads are checksum-pinned. Workflow permissions are read-only, checkout does not retain
credentials, and no repository secrets are required.

A scanner does not prove the absence of secrets or identify all personal data.
Local hooks can be bypassed. CI runs after content is uploaded, so it cannot undo a
public disclosure. Manual review and scanning before the first push are essential.
Hosted secret scanning, push protection, private vulnerability reporting, and
branch/ruleset enforcement are separate repository settings; their presence is
not established by committing this workflow. Maintainers should review the
available settings before accepting contributions.

The synthetic ledger does not implement authentication, encryption, consent,
retention, production tenant isolation or vehicle API security. Those controls
require a separate implementation and security review before any real data is connected.
