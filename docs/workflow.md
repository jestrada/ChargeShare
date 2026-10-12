# How we build features

ChargeShare is built one GitHub issue at a time by Joseph working with a few AI
agents. This page explains the steps each issue goes through.

```
 Wayfinder map (#9)
        |  Joseph picks next issue
        v
 Luna: draft spec + open questions  --->  Joseph answers
                                              |  final spec in issue body
                                              v
 Sol: build in own worktree/branch
        |
        v
 Chargie: review diff, local checks, end-to-end + forced-failure runs
        |
        v
 Browser check: desktop + 375px screenshots on the issue
        |  no browser proof, no PR
        v
 PR "Closes #N"  --->  CI + evidence check  --->  Joseph reviews and merges
```

## Steps

1. **Pick the issue.** The Wayfinder map ([issue #9](https://github.com/jestrada/ChargeShare/issues/9))
   orders the work. Joseph picks the next issue.
2. **Draft the spec.** Luna (Codex `gpt-6-luna`) drafts a spec and a few open
   questions, each with a recommended answer. They are posted on the issue with
   the questions at the top.
3. **Settle the spec.** Joseph answers the questions. The final spec goes into
   the issue body. Every issue opens with a short `## For Joseph`
   summary; the full spec sits in a collapsed details block (see
   [AGENTS.md](../AGENTS.md#issue-and-comment-style)).
4. **Build.** Sol (Codex `gpt-6.1-sol`, xhigh) builds the feature on Grok Bot's
   computer in its own git worktree and branch. Scope stays tight. Sol follows
   [code design](code-design.md) and updates only [architecture](architecture.md),
   and only when the architecture changes.
5. **Review and test.** Grok Bot (Chargie) reviews the diff for scope creep and
   runs all local checks. It also runs a real local end-to-end test, fed by
   synthetic fixtures: Tesla's unmodified fleet-telemetry receiver -> Kafka ->
   Rust ingestion -> SQLite -> API -> dashboard. It adds forced-failure runs to
   prove failures are caught.
6. **Check in a browser.** Screenshots at desktop and 375px phone width are
   posted on the issue in one short result comment (pass/fail, a few bullets,
   details collapsed). Checklist boxes on the issue are
   ticked only where there is real evidence. No browser proof, no PR.
7. **Open the PR.** The PR body says `Closes #N`. CI extends the existing
   workflows; features do not add new workflows. The evidence check fails if the
   linked issue has no screenshot comment. The `no-evidence` label skips it.
8. **Merge.** Joseph reviews and merges. Bots never merge.

## Side tasks

Docs, CI and tooling work goes to Chargie DevEx, in its own worktree. Every
agent labels its PRs with its name, `bot-<name>` (for example `bot-chargie-devex`).
See [AGENTS.md](../AGENTS.md) for the detailed rules.
