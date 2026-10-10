# Repository issue tracker

Use GitHub Issues in `jestrada/ChargeShare` through the `gh` CLI for repository
issue tracking, including Wayfinder maps and decision tickets. This applies to
all agents and skills, including delegated agents and supporting skills invoked
independently. Run commands from this repository or supply
`--repo jestrada/ChargeShare` explicitly.

This document supplies the tracker configuration expected by the bundled skills.
Their generic setup prompts and local-Markdown tracker fallbacks do not apply
here. Pass this guide and the repository name to delegated agents. If `gh` or
GitHub access is unavailable, report the blocker and retain any draft for later
publication; do not establish a separate tracker.

Create or update issues when requested or required by the invoked workflow,
reusing relevant existing issues. Keep implementation scope, checklists and
acceptance evidence in the owning issue and PR. Link enduring contracts and
architecture documentation rather than duplicating them. Small changes can use
one issue or PR without a Wayfinder map.

OpenSpec is paused. Retained files in `openspec/` are reference material and
historical evidence, not an active workflow. Do not create new OpenSpec artifacts
or scaffold changes; record new planning and checklists in GitHub Issues instead.
Pass this restriction to delegated agents. When continuing an existing change,
carry its remaining behavioral acceptance into the owning GitHub issue, linking
the original contract and evidence. Do not require OpenSpec validation, spec sync
or archival, and do not mark historical tasks complete merely to retire the tool.

## Wayfinder trial scope

Invoke `$wayfinder` with the goal to explore or an existing map's issue URL.
Agree on the destination before creating a map. If the question fits one session,
use the skill's short path instead of creating a map and tickets unnecessarily.

Wayfinder records planning decisions. Existing domain contracts still apply;
a planning ticket is not evidence that behavior is implemented.
Keep implementation within the user's approved scope and follow `AGENTS.md`,
`SECURITY.md`, and `docs/code-design.md` for code, verification and publication.

Implementation issues are separate from the map's decision tickets. Link them
from the map's Notes and their PRs; use their own checklists and acceptance
evidence. Do not label delivery work as a Wayfinder decision merely to put it
under the map.

## GitHub operations

- Read an issue with `gh issue view <number> --json number,title,body,labels,assignees,comments,url`.
- List open issues with `gh issue list --state open --json number,title,labels,assignees,url`.
- Create issues with `gh issue create --title "<title>" --body-file <body-file>`.
- Edit issue bodies with `gh issue edit <number> --body-file <body-file>`.
- Post resolution comments with `gh issue comment <number> --body-file <body-file>`.
- Close resolved issues with `gh issue close <number>`.
- Refer to issues by linked title in user-facing text.
- Draft multi-line bodies in files and review their exact content before publishing.
  All issue content must satisfy this public repository's synthetic-data boundary.

## Wayfinding operations

The map is one issue labelled `wayfinder:map`. Its body contains Destination,
Notes, Decisions so far, Not yet specified, and Out of scope. Decisions so far
contains short links to resolved tickets; each ticket holds its own answer.

Each decision ticket is a child issue carrying one of `wayfinder:research`,
`wayfinder:prototype`, `wayfinder:grilling`, or `wayfinder:task`. Create missing
labels only when needed. Use `gh issue create --parent <map-number>` or
`gh issue edit <map-number> --add-sub-issue <child-number>` for the relationship.
If sub-issues are unavailable, use a task list in the map and a
`Part of #<map-number>` line in each child.

Create tickets before adding cross-references or dependencies. Add native blocking
with `gh issue edit <child-number> --add-blocked-by <blocker-number>`.
If dependencies are unavailable, use a `Blocked by: #<number>` line and inspect
whether each blocker remains open.

The frontier consists of the map's open, unassigned children with no open blockers.
Query children with `gh api --paginate repos/jestrada/ChargeShare/issues/<map-number>/sub_issues`;
use `issue_dependencies_summary.blocked_by` when native dependencies are available.
Claim a ticket with `gh issue edit <number> --add-assignee @me` before working on it.
On resolution, post the answer as a comment, close the ticket, then add a linked
one-line pointer to the map. Follow Wayfinder's limit of one resolved decision
ticket per session, except research tickets.

## Skill provenance

The trial installs `wayfinder` and its supporting `grilling`, `domain-modeling`,
`research`, and `prototype` skills from the public
[mattpocock/skills repository](https://github.com/mattpocock/skills/tree/49dd158d1076134a641b33efb035946536778336),
pinned at commit `49dd158d1076134a641b33efb035946536778336`.
Their source files are copied unchanged into `.agents/skills/`, including the
agent metadata and referenced templates. The upstream
[MIT license](../licenses/matt-pocock-skills-MIT.txt) is retained.

Installation used `gh api` to read the pinned source. Updates are deliberate;
review upstream changes and update this provenance together with the skill files.
