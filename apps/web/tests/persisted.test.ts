import { test } from "node:test"
import assert from "node:assert/strict"
import { classificationLabel, reasonLabel, evidenceLabel, parseResults, resultPath, scopes } from "../src/persisted.ts"

function result() {
  return {
    ...scopes[0], state: "held", evidence_boundary: "Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated",
    observed_ac_kwh: "0.500000", eligible_shared_charger_kwh: "0.000000", revision: "results-v1-0123456789abcdef",
    provenance: { source: "synthetic-receiver-kafka", source_epoch: "synthetic-epoch", input_contract: "input-v1", mapping_version: "mapping-v1", manifest_version: "manifest-v1", normalizer_version: "normalizer-v1", calculation_version: "scoped-core-replay-v2", geofence_version: "fictional-geofence-v1" },
    sessions: [{ vehicle: "vehicle-a", connection: "connection-1", classification: "Held", classification_reason: "LocationUnavailable", evidence_state: "complete", observed_ac_kwh: "0.500000", eligible_shared_charger_kwh: "0.000000", quality_flags: [], exclusion_reasons: ["LocationUnavailable"] }],
  }
}

test("persisted client only constructs an explicitly scoped persisted route", () => {
  assert.equal(resultPath(scopes[0]), "/api/poc/owners/owner-a/vehicles/vehicle-a/results")
  assert.equal(resultPath({ owner: "a/b", vehicle: "v?" }), "/api/poc/owners/a%2Fb/vehicles/v%3F/results")
})
test("scoped energy strings are preserved and another scope is refused", () => {
  assert.equal(parseResults(result(), scopes[0]).observed_ac_kwh, "0.500000")
  assert.throws(() => parseResults(result(), scopes[1]))
  const changed = result()
  changed.sessions[0].vehicle = "vehicle-b"
  assert.throws(() => parseResults(changed, scopes[0]))
})
test("empty and valid zero evidence remain distinct from incomplete held evidence", () => {
  const empty = { ...result(), state: "empty", sessions: [], observed_ac_kwh: "0.000000" }
  assert.equal(parseResults(empty, scopes[0]).state, "empty")
  const zero = result()
  zero.observed_ac_kwh = "0.000000"
  zero.sessions[0].observed_ac_kwh = "0.000000"
  zero.sessions[0].evidence_state = "complete_zero"
  assert.equal(evidenceLabel(parseResults(zero, scopes[0]).sessions[0]), "Complete zero-energy evidence")
  zero.sessions[0].evidence_state = "incomplete"
  assert.equal(evidenceLabel(parseResults(zero, scopes[0]).sessions[0]), "Incomplete evidence")
})
test("missing or fixture shaped responses cannot become available persisted results", () => {
  for (const value of [null, {}, { demo: true }, { ...result(), observed_ac_kwh: 0.5 }, { ...result(), eligible_shared_charger_kwh: "0.500000" }, { ...result(), state: "empty" }]) {
    assert.throws(() => parseResults(value, scopes[0]))
  }
})

test("automatic classifications and holds display their reasons and eligibility", () => {
  for (const [classification, reason, exclusions, eligible, state] of [
    ["SharedCharger", "InsideGeofence", [], "0.500000", "eligible"],
    ["OtherCharger", "OutsideGeofence", ["OtherCharger"], "0.000000", "held"],
    ["Held", "LocationUnavailable", ["LocationUnavailable"], "0.000000", "held"],
    ["Held", "LocationAmbiguous", ["LocationAmbiguous"], "0.000000", "held"],
  ] as const) {
    const value = result()
    Object.assign(value, { state, eligible_shared_charger_kwh: eligible })
    Object.assign(value.sessions[0], { classification, classification_reason: reason, exclusion_reasons: exclusions, eligible_shared_charger_kwh: eligible })
    const parsed = parseResults(value, scopes[0])
    assert.equal(parsed.sessions[0].classification_reason, reason)
    assert.equal(parsed.eligible_shared_charger_kwh, eligible)
    assert.ok(reasonLabel(reason).includes(reason === "InsideGeofence" ? "inside" : reason === "OutsideGeofence" ? "outside" : "Held automatically"))
    assert.ok(classificationLabel(parsed.sessions[0]).startsWith(classification))
  }
})
test("shared charger location cannot override quality or charge type exclusions", () => {
  const value = result()
  Object.assign(value.sessions[0], { classification: "SharedCharger", classification_reason: "InsideGeofence", exclusion_reasons: ["DcEvidence"] })
  assert.equal(classificationLabel(parseResults(value, scopes[0]).sessions[0]), "SharedCharger · Held")
  value.sessions[0].exclusion_reasons = []
  value.sessions[0].quality_flags = ["ConflictingEvidence"]
  assert.equal(classificationLabel(parseResults(value, scopes[0]).sessions[0]), "SharedCharger · Held")
  value.sessions[0].eligible_shared_charger_kwh = "0.500000"
  assert.throws(() => parseResults(value, scopes[0]))
})
test("coordinate fields and coordinate text cannot enter persisted display contracts", () => {
  const value = result()
  for (const changed of [
    { ...value, latitude: 12.3456789 },
    { ...value, sessions: [{ ...value.sessions[0], location: { latitude: 12.3456789 } }] },
    { ...value, sessions: [{ ...value.sessions[0], classification_reason: "12.3456789" }] },
    { ...value, provenance: { ...value.provenance, longitude: 45.6789123 } },
    { ...value, evidence_boundary: value.evidence_boundary + " 12.3456789" },
  ]) assert.throws(() => parseResults(changed, scopes[0]))
})
