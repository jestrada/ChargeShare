# Tasks

## 1. Scope review

- [ ] 1.1 Obtain explicit approval to implement this proposal in the review conversation; record the approved scope before changing application code. Specification and implementation can remain in this same PR.

## 2. Validated profiles and schedule resolution

- [ ] 2.1 Add strict, bounded rate-profile decoding and date/version validation in the preview crate; verify synthetic tests for invalid dates, overlaps, duplicate keys/IDs, unknown fields, invalid decimals, size limits and daily coverage. Document the schema with a fictional example.
- [ ] 2.2 Compile intersecting effective dates into the fixed sample timeline and coalesce only compatible adjacent windows; verify version boundaries, uncovered dates, reordered versions, horizon limits and host-timezone independence. Document the synthetic clock boundary.
- [ ] 2.3 Feed compiled schedules into existing scoped ledger quotes; verify unchanged evidence/identities, corrected-rate repricing, restored-profile determinism, version attribution, existing whole-session holds and exact subtotal rounding. Keep the default expected totals unchanged.

## 3. Explicit local loading

- [ ] 3.1 Add startup-only `--rate-file PATH` support and launcher argument forwarding; verify default operation, a valid synthetic file and safe failure before binding for invalid files/arguments. Document startup, restart and rollback commands using an external fictional file path.
- [ ] 3.2 Add source/effective-date metadata and unavailable outlook values to the preview response; verify both scenario routes, rate gaps, next-window contiguity and existing request restrictions without returning source paths or raw JSON. Document the response changes.

## 4. Dashboard presentation

- [ ] 4.1 Update response types and render local historical-rate labels, sample-clock wording, exact detailed rates and explicit unavailable states; verify with a production frontend build and browser checks of fictional default, local and missing-coverage modes. Document that these are sample costs, not actual spending or amounts owed.
- [ ] 4.2 Check desktop and mobile screenshots, keyboard filtering/disclosures and help text using synthetic profiles; fix overflow or unclear provenance and retain verification evidence outside committed source.

## 5. Integration and publication

- [ ] 5.1 Run strict OpenSpec validation, Rust formatting/Clippy/workspace tests and the frontend build after implementation; verify existing acceptance scenarios still pass and record the results in the PR.
- [ ] 5.2 Run staged/history privacy scans and guard tests, inspect the complete staged diff and author metadata, then push the implementation to this PR; verify its remote commit and terminal hosted checks. Keep all household profiles, bills and derived private calculations out of publication.

Live setup, hosting, Tesla authorization and deployment are outside this change.
No tasks for those actions are implied by completing this checklist.
