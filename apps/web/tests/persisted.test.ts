import { readFileSync } from "node:fs"
import { test } from "node:test"
import assert from "node:assert/strict"
import { classificationLabel, reasonLabel, evidenceLabel, parseResults, resultPath, scopes, pricingBoundary, pricingHoldLabel } from "../src/persisted.ts"

function result() {
  return {
    ...scopes[0], state: "held", evidence_boundary: "Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated",
    observed_ac_kwh: "0.500000", eligible_shared_charger_kwh: "0.000000", revision: "results-v1-0123456789abcdef",
    provenance: { source: "synthetic-receiver-kafka", source_epoch: "synthetic-epoch", input_contract: "input-v1", mapping_version: "mapping-v1", manifest_version: "manifest-v1", normalizer_version: "normalizer-v1", calculation_version: "scoped-core-replay-v2", geofence_version: "fictional-geofence-v1" },
    pricing: { state: "held", charger_id: null, rate_models: [], calculation_version: "tou-linear-micro-kwh-v1", estimate_boundary: pricingBoundary, priced_ac_kwh: "0.000000", exact_subtotal: null, display_subtotal: null, quoted_session_count: 0, rate_held_session_count: 0, ineligible_session_count: 1 },
    sessions: [{ vehicle: "vehicle-a", connection: "connection-1", classification: "Held", classification_reason: "LocationUnavailable", evidence_state: "complete", observed_ac_kwh: "0.500000", eligible_shared_charger_kwh: "0.000000", quality_flags: [], exclusion_reasons: ["LocationUnavailable"], pricing: { state: "ineligible", hold_reason: "IneligibleSession", quote: null } }],
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
  const empty = { ...result(), state: "empty", sessions: [], observed_ac_kwh: "0.000000", pricing: { ...result().pricing, state: "empty", ineligible_session_count: 0 } }
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
    if (classification === "SharedCharger") {
      Object.assign(value.sessions[0].pricing, { state: "rate_held", hold_reason: "MissingRate" })
      Object.assign(value.pricing, { rate_held_session_count: 1, ineligible_session_count: 0 })
    }
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

function quotedResult() {
  const value: any = result()
  value.state = "eligible"
  value.eligible_shared_charger_kwh = "0.500000"
  value.pricing = { ...value.pricing, state: "quoted", charger_id: "fictional-shared-charger", rate_models: JSON.parse(readFileSync(new URL("../../../dev/ingestion-manifest.json", import.meta.url), "utf8")).rate_models,
    priced_ac_kwh: "0.500000", exact_subtotal: "0.1127900000000000 USD", display_subtotal: "$0.11", quoted_session_count: 1, ineligible_session_count: 0 }
  Object.assign(value.sessions[0], { classification: "SharedCharger", classification_reason: "InsideGeofence", eligible_shared_charger_kwh: "0.500000", exclusion_reasons: [],
    pricing: { state: "quoted", hold_reason: null, quote: { exact_cost: "0.1127900000000000 USD", display_cost: "$0.11", slices: [{ start_utc_seconds: "1788264000", end_utc_seconds: "1788264600", local_date: "2026-09-01", local_start: "2026-09-01T05:00:00-07:00", local_end: "2026-09-01T05:10:00-07:00", utc_offset_seconds: "-25200", season: "Summer", day_type: "Weekday", period: "off_peak", model_version: "pge-ev2-a-2026-03-v1", allocated_ac_kwh: "0.500000", usd_per_kwh: "0.2255800000 USD/kWh", exact_cost: "0.1127900000000000 USD" }] } } })
  return value
}

test("EV2-A price and provenance strings reach the client unchanged", () => {
  const value = quotedResult()
  const parsed = parseResults(value, scopes[0])
  assert.equal(parsed.pricing.exact_subtotal, "0.1127900000000000 USD")
  assert.equal(parsed.sessions[0].pricing.quote?.display_cost, "$0.11")
  assert.equal(parsed.sessions[0].pricing.quote?.slices[0].usd_per_kwh, "0.2255800000 USD/kWh")
  assert.equal(parsed.pricing.rate_models[0].plan, "PGE-EV2-A")
  assert.equal(parsed.pricing.rate_models[0].periods.some(period => period.period === "part_peak"), true)
})
test("rate-held, zero and empty pricing are distinct and never fabricate an amount", () => {
  const value = quotedResult()
  value.sessions[0].pricing = { state: "rate_held", hold_reason: "MissingRate", quote: null }
  Object.assign(value.pricing, { state: "held", quoted_session_count: 0, rate_held_session_count: 1, priced_ac_kwh: "0.000000", exact_subtotal: null, display_subtotal: null })
  assert.equal(parseResults(value, scopes[0]).sessions[0].pricing.quote, null)
  assert.match(pricingHoldLabel("MissingRate"), /no selected rate coverage/)
  const zero = quotedResult()
  Object.assign(zero, { observed_ac_kwh: "0.000000", eligible_shared_charger_kwh: "0.000000" })
  Object.assign(zero.sessions[0], { observed_ac_kwh: "0.000000", eligible_shared_charger_kwh: "0.000000", evidence_state: "complete_zero" })
  zero.sessions[0].pricing.quote = { exact_cost: "0.0000000000000000 USD", display_cost: "$0.00", slices: [] }
  Object.assign(zero.pricing, { priced_ac_kwh: "0.000000", exact_subtotal: "0.0000000000000000 USD", display_subtotal: "$0.00", rate_models: [] })
  assert.equal(parseResults(zero, scopes[0]).pricing.display_subtotal, "$0.00")
})
test("partial, unsafe, numeric and inconsistent pricing responses are refused", () => {
  for (const change of [
    (value: any) => { value.pricing.exact_subtotal = 0.11279 },
    (value: any) => { value.sessions[0].pricing.quote.display_cost = 0.11 },
    (value: any) => { value.sessions[0].pricing.quote.slices[0].latitude = 12.3456789 },
    (value: any) => { value.sessions[0].pricing.quote.slices[0].model_version = "neighbor-version" },
    (value: any) => { value.pricing.quoted_session_count = 2 },
    (value: any) => { value.sessions[0].pricing.state = "rate_held" },
    (value: any) => { value.sessions[0].exclusion_reasons = ["DcEvidence"]; value.sessions[0].eligible_shared_charger_kwh = "0.000000"; value.state = "held"; value.eligible_shared_charger_kwh = "0.000000" },
  ]) { const value = quotedResult(); change(value); assert.throws(() => parseResults(value, scopes[0])) }
})
