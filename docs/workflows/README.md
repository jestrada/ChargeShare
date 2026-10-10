# Receiver workflow verification

The harness source was recovered from commit
`ec1c8922c813e8af57ea931324466ccc5afba5d5` and published on PR #5 without
rewriting history. The earlier environment could not publish workflow changes.
The current local GitHub login has the required workflow scope, so the recovered
workflow is now active at
[telemetry-integration.yml](../../.github/workflows/telemetry-integration.yml).

The initial activation retains `CHARGESHARE_CI_PROVE_FAILURE: '1'`. Its intended
run deliberately fails after a successful receiver/Kafka smoke to prove the
missing-output gate; a setup failure does not prove that gate. Verify its
stage summary and successful owned-resource teardown before removing that job's
`env` block. A subsequent normal run must pass on its exact published commit.

Existing repository checks remain required. Activation does not prove startup,
transport, retention or lifecycle acceptance, and unchecked OpenSpec tasks stay
unchecked until their stated runtime evidence exists. Only allowlisted synthetic
summaries, versions and normalized fixture results are uploaded. Keys,
certificates and unrestricted runtime logs are excluded.
