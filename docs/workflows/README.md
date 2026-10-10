# Receiver workflow verification

The harness source was recovered from commit
`ec1c8922c813e8af57ea931324466ccc5afba5d5` and published on PR #5 without
rewriting history. The earlier environment could not publish workflow changes.
The current local GitHub login has the required workflow scope, so the recovered
workflow is now active at
[telemetry-integration.yml](../../.github/workflows/telemetry-integration.yml).

Activation commit `4ea28f01445265b8e9e630cccc848895847a6d59` retained
`CHARGESHARE_CI_PROVE_FAILURE=1`. The [push run](https://github.com/jestrada/ChargeShare/actions/runs/38076460351)
and [PR run](https://github.com/jestrada/ChargeShare/actions/runs/38076463792)
both passed the authenticated receiver smoke, then failed the deliberately
missing-output multiset assertion. Both completed cleanup and uploaded only the
allowlisted summary and versions. The setting is now removed; full passing
acceptance remains required before tasks are marked complete.

Existing repository checks remain required. Activation does not prove startup,
transport, retention or lifecycle acceptance, and unchecked OpenSpec tasks stay
unchecked until their stated runtime evidence exists. Only allowlisted synthetic
summaries, versions and normalized fixture results are uploaded. Keys,
certificates and unrestricted runtime logs are excluded.
