# ChargeShare contributor guidance

This public repository contains only synthetic charging evidence and labeled
sample rates. Before editing, read [README](README.md), [SECURITY](SECURITY.md),
the owning GitHub issue and [architecture](docs/architecture.md).

## Design and working rules

- Read and follow [code design](docs/code-design.md) during the design phase,
  before writing code. Use expressive domain names, explicit types, cohesive
  functions, small public facades and inward dependencies. Introduce another
  pattern only for a concrete approved need, explaining the trade-off in
  `docs/architecture.md`, `docs/code-design.md` or the review.
- Do not add comments to authored code, including Rust doc comments. Express
  intent through names, types, structure and tests; put enduring rationale and
  contracts in `docs/architecture.md` or `docs/code-design.md`. Preserve mandatory
  notices/generated requirements and surface conflicts as the design guide requires.
- Keep backend/domain logic and money calculations in Rust. The authorized
  `apps/web` preview uses React/TypeScript, shadcn and Node/npm tooling.
- Establish the problem, approved scope and observable acceptance criteria in
  the owning issue before implementing new behavior. Keep proposals distinct
  from implementation and unimplemented tasks unchecked.
- Keep changes inside this repository. Never import private code, correspondence,
  live vehicle data, secrets, real VINs/locations, bills or account/contact details.
  Use fictional vehicles and explicitly labeled public sample tariffs only.
- Registration, key pairing, OAuth, deployment, spending and vehicle-control
  features need a separate explicit request.
- Every energy total must state its measurement/estimate boundary and quality;
  never claim utility-meter accuracy without evidence.
- For each PR, update `docs/architecture.md` and its Mermaid diagram only when
  components, data flow, boundaries or architectural decisions actually change.
  Otherwise state **"no architecture change"** in the issue summary. Fix affected
  README/contributor guidance when needed; do not create new documentation files
  without Joseph's approval. Repository-local skills follow this restriction too.
- Test evidence and run results go only in the owning GitHub issue or PR, never
  repository documentation. Keep architecture/design prose about enduring contracts.
- Run applicable [README checks](README.md#tests) and [security checks](SECURITY.md#before-every-publication)
  before committing. Review exact staged paths/diff and scanner output; keep hooks
  enabled and never bypass failures.
- Use `gh stack` for every repository change/PR, following the workflow below.
  Create or switch to a dedicated stack branch before editing; do not commit or
  push to `main` without an explicit exception for the current change.

## Branches and stacked pull requests

All agents and skills use the official [gh stack extension](https://github.com/github/gh-stack).
Independent work starts at current `main`; dependent increments extend a stack,
with each PR targeting the branch below and the bottom PR targeting `main`.

- If missing, install with `gh extension install github/gh-stack`; verify
  `gh stack --version`. Read command help before unfamiliar flags. Report blockers
  instead of silently substituting manual branch/PR management.
- Before editing, fetch remote refs and inspect `gh pr list --repo jestrada/ChargeShare`.
  Inspect tracked stacks with `gh stack view --json`; continue with
  `gh stack checkout <PR-or-branch>`. Adopt manual chains with
  `gh stack link <bottom-PR> <next-PR> ...`, then check out through the tool.
- Start independent work with `gh stack init --base main <branch>` or dependent
  work from the stack top with `gh stack add <branch>`. Keep layers cohesive.
- Stage reviewed paths and commit normally after checks. Avoid automatic stage-all.
- After changing a parent or updating `main`, run `gh stack rebase`. Resolve in
  the reported worktree, stage reviewed resolutions and use `gh stack rebase --continue`;
  use `gh stack rebase --abort` when needed.
- Publish drafts with `gh stack submit --auto`; update existing branches with
  `gh stack push`. Use submit when bases/membership change. Respect explicit lease
  protection: resolve rejected leases, never use unconditional force pushes.
- After a parent merges, run `gh stack sync`, preserving implementation/acceptance
  status; inspect affected diffs/bases and rerun applicable checks.
- Verify every affected remote head, PR base and terminal hosted check before
  reporting completion, including partial-push recovery. Keep metadata local and
  pass this workflow to delegated agents.

## Agent authorship labels

Every agent that opens a pull request must apply a label naming itself so Joseph
can track authorship. Use `bot-<agent-name>` in kebab-case (for example
`bot-chargie` or `bot-chargie-devex`). Create the label with `gh label create`
if it does not already exist, then apply it to the PR.

## Issue tracking

All agents and skills use GitHub Issues in `jestrada/ChargeShare` through `gh`.
Run from this repository or supply `--repo jestrada/ChargeShare`. This tracker is
configured: no generic setup prompts, separate trackers or local-Markdown fallback.
Reuse relevant issues; if access fails, report the blocker and retain a draft.
Pass the repository and this guide to delegated agents.

Read issues with `gh issue view <number> --json number,title,body,comments,url`.
Use reviewed files with `--body-file` for multiline issue/PR bodies or comments.
Keep current scope, implementation checklists and acceptance evidence in issues
and PRs; link architecture/design contracts rather than copying them. Close
implementation issues only after observable acceptance is verified.

### Human-readable change reviews

Every PR has a current summary in its owning issue body or an identified comment.
The PR links directly to that summary, and the summary links back to the PR.
Explain the problem, what changed and why, before/after behavior where useful,
scope, checks/results, review steps, exact-head CI/acceptance evidence, unrun
checks, limitations and remaining work. Link affected architecture/diagram and
guide changes, or state "no architecture change" and explain any other omitted
guide update. Distinguish implemented behavior from proposals. Each PR sharing
an issue gets its own summary/evidence. Refresh after material changes and before
final review; earlier runs do not verify later commits. Videos are optional and
deferred. All review artifacts must respect SECURITY.md.

### Wayfinding operations

User-invoked Wayfinder uses `.agents/skills/` for exploratory planning. Agree on
the destination first; small work needs no map. Implementation issues stay
separate from decision tickets and need approved scope and acceptance criteria.

- A `wayfinder:map` issue contains Destination, Notes, Decisions so far, Not yet
  specified and Out of scope. Link implementation issues/PRs from Notes.
- Decision children use `wayfinder:research`, `wayfinder:prototype`,
  `wayfinder:grilling` or `wayfinder:task`. Create tickets before cross-references.
  Use `gh issue create --parent <map-number>` or
  `gh issue edit <map-number> --add-sub-issue <child-number>`; if unsupported,
  use a map task list plus `Part of #<map-number>` in each child.
- Add blocking with `gh issue edit <child-number> --add-blocked-by <blocker-number>`;
  fallback is `Blocked by: #<number>` with blocker-state inspection.
- Query children with `gh api --paginate repos/jestrada/ChargeShare/issues/<map-number>/sub_issues`.
  The frontier is open, unassigned children with no open blockers; inspect
  `issue_dependencies_summary.blocked_by` when available. Claim with
  `gh issue edit <number> --add-assignee @me` before work.
- Resolve by commenting the answer, closing the ticket and adding a linked
  one-line map pointer. Follow the skill's one-decision-per-session limit,
  except research tickets. Planning resolution is not delivery evidence.

Publish new specs through the repository-local `to-spec` skill only when requested.
Use the existing `enhancement` triage label instead of `ready-for-agent`; do not
create that label or apply a different triage label.

Do not use OpenSpec. It was removed; do not add `openspec/` files, proposals or
validation/sync/archive steps, including through skills or delegation.

Bundled Matt Pocock skills come from the public
[pinned source](https://github.com/mattpocock/skills/tree/49dd158d1076134a641b33efb035946536778336);
installation used `gh api`. Review deliberate upstream updates with the skill
files and this provenance. Preserve the [MIT notice](docs/licenses/matt-pocock-skills-MIT.txt).

## PR evidence check

The `evidence-check` job in `.github/workflows/ingestion-acceptance.yml` runs on
pull requests only (never on pushes to `main`). When a PR body links an issue with
`Closes`/`Fixes`/`Resolves #N` (any tense, case-insensitive), post a comment on each
linked issue with at least one screenshot/image (`![..](..)` or `<img>`) after
starting the PR. If evidence does not apply, apply the `no-evidence` label instead.
