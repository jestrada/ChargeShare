# Issue #27 hosted gate handoff

The publishing token cannot modify workflows. The maintainer must move
`ingestion-acceptance.yml` to `.github/workflows/ingestion-acceptance.yml` before
the dedicated Ubuntu 24.04 job can run. This staged file is inactive.

Run the normal gate and dispatch separate `setup`, `assertion`, and `timeout`
failure proofs. Each must fail its acceptance step and complete owned cleanup.
Dispatch `none` in a separate restored run and record its passing result, the
exact published commit and terminal checks in issue #27. The assertion mode
deliberately expects vehicle A to have 0.6 kWh instead of 0.5 kWh. No mode masks
failure with `continue-on-error`.

Artifacts contain only the listed summaries and tool versions. The private
decoded capture, unrestricted logs, SQLite files and generated test trust are
excluded. Review `.cache/ingestion-acceptance/decoded-capture.json` privately from
a successful real run and add a reviewed fictional golden fixture when resuming
implementation. Candidate fixtures do not establish actual receiver output.
