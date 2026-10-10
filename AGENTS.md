# ChargeShare contributor guidance

This is a public repository with an offline synthetic Rust ledger and pricing. Read README.md, SECURITY.md and the applicable OpenSpec contract and change before editing.

## Overall code design goal

Write self-documenting, agent-friendly code: expressive domain names, explicit types and boundaries, cohesive functions, a predictable module layout and limited hidden side effects. New code should follow the established simple patterns; introduce a different pattern only for a concrete approved need and explain the trade-off in repository documentation or review.

Do not add comments to authored code, including Rust doc comments. Make intent clear through names, types, structure and tests, and keep rationale and contracts in repository documentation. If mandatory notices, generated conventions or safety/tool requirements conflict, preserve required content and ask for a decision as described in [the design guide](docs/code-design.md).

## Working rules

- Create or switch to a dedicated branch before editing, and commit and push that branch. Do not commit or push directly to `main` unless the user explicitly authorizes an exception for the current change.
- Keep domain logic and backend source in Rust. The user-authorized localhost preview uses React/TypeScript and shadcn in `apps/web`; Node.js/npm runs its build/dev tooling and OpenSpec. Keep money calculations in the Rust core.
- Read [docs/code-design.md](docs/code-design.md) before code or architecture changes. Keep `lib.rs` a small public facade, use cohesive modules and inward domain dependencies, and follow the guide's required decision/review workflow within the approved OpenSpec scope.
- Keep changes inside this repository. Do not import private repositories, personal correspondence or live vehicle data.
- Use synthetic vehicle examples and the explicitly labeled public sample tariff only. Never commit secrets, real VINs, home locations, bills, receipts, account identifiers or contact details.
- Do not register a Tesla app, pair keys, authorize OAuth, deploy, incur costs, or add vehicle-control features without a separate explicit request.
- Preserve the distinction between proposed behavior and implemented functionality. Keep unimplemented OpenSpec tasks unchecked.
- Use the generated OpenSpec skills under .agents/skills. Start with a proposal and scenarios, then implement an approved change.
- Run npm run spec:validate and the security checks described in SECURITY.md before a commit. Review the exact staged diff and paths as well as scanner output. Do not bypass failing hooks.
- Treat every energy total as a measured or estimated quantity with a stated boundary and quality. Never advertise utility-meter accuracy without evidence.

## Issue tracking

All agents and skills working in this repository use GitHub Issues in
`jestrada/ChargeShare` through `gh` whenever issue tracking is needed. Read
[the tracker conventions](docs/agents/issue-tracker.md) before tracker operations,
and pass the repository and guide path to delegated agents. This tracker is
already configured; generic skill setup prompts and local-Markdown tracker
fallbacks do not apply here. Keep OpenSpec artifacts and implementation checklists
in `openspec/`, linking them from related issues.

## Wayfinder trial

User-invoked Wayfinder sessions use the repository-local Matt Pocock skills in
`.agents/skills/`. Track their maps and decision tickets using the issue-tracking
conventions above.
Wayfinder is approved for exploratory planning. New behavior still needs an
approved implementation scope and the applicable domain contracts and checks.

Read the existing architecture, pricing and preview documentation for domain
language. If `GLOSSARY.md` or relevant records in `docs/adr/` exist, read those too;
create them only when a resolved term or enduring decision needs a record.
