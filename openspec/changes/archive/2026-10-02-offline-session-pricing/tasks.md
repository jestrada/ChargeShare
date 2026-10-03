# Tasks

## 1. Exact pricing inputs

- [x] 1.1 Implement exact USD rates/costs and validated versioned rate windows; document their public contracts and verify parsing, numeric limits, rounding and window validation tests.

## 2. Evidence-derived session pricing

- [x] 2.1 Retain internal counter segments and add a scoped pricing query with complete quotes, explicit holds and exact priced subtotals; document behavior and verify all pricing acceptance scenarios plus unchanged Spec 1 tests.

## 3. Runnable demonstration and integration checks

- [x] 3.1 Add a fictional executable example and require both acceptance suites in the existing CI wrapper; update status/usage documentation, run the example, formatting, Clippy, complete offline suite, strict specs and security checks, and review the final diff for private data and unintended changes.

Live setup, monthly statements, deployment and publication are not tasks in this change.
