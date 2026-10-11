# ChargeShare contributor guidance

This is a public repository with an offline synthetic Rust ledger and pricing. Read README.md, SECURITY.md, the relevant GitHub issue and the applicable domain documentation before editing.

## Overall code design goal

Write self-documenting, agent-friendly code: expressive domain names, explicit types and boundaries, cohesive functions, a predictable module layout and limited hidden side effects. New code should follow the established simple patterns; introduce a different pattern only for a concrete approved need and explain the trade-off in repository documentation or review.

Do not add comments to authored code, including Rust doc comments. Make intent clear through names, types, structure and tests, and keep rationale and contracts in repository documentation. If mandatory notices, generated conventions or safety/tool requirements conflict, preserve required content and ask for a decision as described in [the design guide](docs/code-design.md).

## Working rules

- Use `gh stack` for every repository change and pull request, following the stacked workflow below. Create or switch to a dedicated stack branch before editing, then commit and publish through that workflow. Do not commit or push directly to `main` unless the user explicitly authorizes an exception for the current change.
- Keep domain logic and backend source in Rust. The user-authorized localhost preview uses React/TypeScript and shadcn in `apps/web`; Node.js/npm runs its build/dev tooling. Keep money calculations in the Rust core.
- Read [docs/code-design.md](docs/code-design.md) before code or architecture changes. Keep `lib.rs` a small public facade, use cohesive modules and inward domain dependencies, and follow the guide's required decision/review workflow within the approved scope.
- Keep changes inside this repository. Do not import private repositories, personal correspondence or live vehicle data.
- Use synthetic vehicle examples and the explicitly labeled public sample tariff only. Never commit secrets, real VINs, home locations, bills, receipts, account identifiers or contact details.
- Do not register a Tesla app, pair keys, authorize OAuth, deploy, incur costs, or add vehicle-control features without a separate explicit request.
- Preserve the distinction between proposed behavior and implemented functionality. Keep unimplemented tasks unchecked and record acceptance evidence in the owning issue or PR. Keep affected architecture diagrams current in the same PR, following [the design guide](docs/code-design.md#keep-architecture-diagrams-current).
- Give every PR a human-readable change summary in its owning GitHub issue and link directly to that summary from the PR. Explain what changed and why, before/after behavior, verification evidence and remaining limitations; keep it current as the PR changes. Follow the [review summary guidance](docs/agents/issue-tracker.md#human-readable-change-reviews).
- Review documentation impact for every PR. Update affected files in `docs/` and other relevant guides in the same PR so they describe the delivered behavior. Update or add diagrams when architecture, dependencies, data flow or user flows change. Link the docs and diagrams from the issue summary; if either needs no change, state why. Video demos are optional and deferred, not a completion requirement.
- Establish the problem, approved scope and observable acceptance criteria in the relevant issue before implementing new behavior. Use Wayfinder for large planning efforts with unresolved decisions; small changes do not need a map.
- Run the applicable checks described in docs/testing.md and the security checks described in SECURITY.md before a commit. Review the exact staged diff and paths as well as scanner output. Do not bypass failing hooks.
- Treat every energy total as a measured or estimated quantity with a stated boundary and quality. Never advertise utility-meter accuracy without evidence.

## Branches and stacked pull requests

All agents and repository skills use the official
[`gh stack` extension](https://github.com/github/gh-stack) for branch and PR
management. Independent work starts its own stack rooted at `main`; small work
can use a single branch. Dependent increments extend an existing stack, with
each PR targeting the branch immediately below it and the bottom PR targeting
`main`.

- If the extension is missing, install it with `gh extension install github/gh-stack` and verify `gh stack --version`. Read the relevant command's `--help` before using unfamiliar flags. Report a tooling blocker instead of silently replacing the stack workflow with manual PR management.
- Before editing, fetch current remote refs and inspect open PRs with `gh pr list --repo jestrada/ChargeShare`. Inspect a tracked stack with `gh stack view --json`; continue existing work with `gh stack checkout <PR-or-branch>`. Adopt an existing manual PR chain with `gh stack link <bottom-PR> <next-PR> ...`, then check it out through `gh stack`.
- Start independent work from current `main` with `gh stack init --base main <branch>`. Add dependent work from the existing stack's top with `gh stack add <branch>`. Keep each layer cohesive and independently reviewable.
- Stage only reviewed paths and commit normally after the applicable checks and security scans. Keep hooks enabled; avoid the stack command's automatic stage-all shortcuts.
- After changing a lower layer or updating `main`, restack affected descendants with `gh stack rebase`. Resolve conflicts in the reported worktree, stage the reviewed resolutions and use `gh stack rebase --continue`; use `gh stack rebase --abort` to restore the stack when necessary.
- Publish new PRs with `gh stack submit --auto`, which creates drafts. Update existing branches with `gh stack push`; use `gh stack submit --auto` when PR bases or stack membership also need updating. These commands use explicit lease protection for rewritten history. Never use unconditional force pushes or bypass a rejected lease.
- After a parent PR merges, reconcile the remaining stack with `gh stack sync`. Preserve existing implementation and acceptance status, inspect every affected diff/base, and rerun applicable checks on the resulting commits.
- Verify every affected remote head, PR base and terminal hosted check before reporting completion. Stack pushes can partially succeed; resolve any rejected branch and verify the whole stack. Keep stack metadata local, and pass this workflow to delegated agents.

## Issue tracking

All agents and skills working in this repository use GitHub Issues in
`jestrada/ChargeShare` through `gh` whenever issue tracking is needed. Read
[the tracker conventions](docs/agents/issue-tracker.md) before tracker operations,
and pass the repository and guide path to delegated agents. This tracker is
already configured; generic skill setup prompts and local-Markdown tracker
fallbacks do not apply here. Keep current scope, implementation checklists and
acceptance evidence in the owning issues and PRs. Keep enduring domain contracts
and architecture decisions in repository documentation, linking rather than
duplicating them.

Specs for new work are published as GitHub issues through the repository-local
`to-spec` skill, following the tracker conventions.

OpenSpec is paused. Do not invoke its workflow or require proposal, validation,
sync or archive steps. Do not create new OpenSpec change scaffolds, proposals,
designs, specs, task lists or archive copies, including through delegated agents
or automatic skill suggestions. Existing `openspec/` files are retained as reference and
historical evidence; their instructions do not govern current work. Preserve
their recorded status and applicable domain requirements without treating
unimplemented proposals as delivered. Resume OpenSpec only if the user explicitly
requests it.

## Wayfinder trial

User-invoked Wayfinder sessions use the repository-local Matt Pocock skills in
`.agents/skills/`. Track their maps and decision tickets using the issue-tracking
conventions above.
Wayfinder is approved for exploratory planning. New behavior still needs an
approved implementation scope and the applicable domain contracts and checks.

Read the existing architecture, pricing and preview documentation for domain
language. If `GLOSSARY.md` or relevant records in `docs/adr/` exist, read those too;
create them only when a resolved term or enduring decision needs a record.
