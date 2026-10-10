# Receiver workflow handoff

The latest harness source was recovered from commit
`ec1c8922c813e8af57ea931324466ccc5afba5d5`. This handoff is a new fast-forward
commit on PR #5's published base, preserving that source without changing any
`.github/workflows` file in its pushed history. Existing repository checks still
run; receiver/Kafka acceptance has not run.

`telemetry-integration.yml.disabled` is the exact recovered workflow, stored here
and inactive. To activate it yourself with a credential permitted to edit
workflows:

```sh
mkdir -p .github/workflows
mv docs/workflows/telemetry-integration.yml.disabled .github/workflows/telemetry-integration.yml
```

The recovered file sets `CHARGESHARE_CI_PROVE_FAILURE: '1'`. Its first intended
run deliberately fails after a successful receiver/Kafka smoke to prove the
failure gate; a setup failure does not prove that gate. Then remove that job's
`env` block, commit/push again, and require a passing exact-head integration run.
Update the two inactive-workflow links in the development/testing guides when
moving the file. This handoff does not prove startup, transport, retention or
SQLite integration, and does not complete unchecked OpenSpec tasks.
