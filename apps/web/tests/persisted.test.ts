import { test } from "node:test"
import assert from "node:assert/strict"
import { classificationLabel, reasonLabel, evidenceLabel, observationTime, parseResults, parseScopes, resultPath, scopes, pricingBoundary, livePricingBoundary, liveQuantization, pricingHoldLabel, dayLabel, chargingPeriods } from "../src/persisted.ts"

const zeroTotal = { ac_kwh: "0.000000", exact_cost: "0.0000000000000000 USD", display_cost: "$0.00" }
const emptyEvidence = { quoted_sessions: 0, held_sessions: 0, other_sessions: 0, partial_sessions: 0, complete_zero_sessions: 0, quality_flags: [] as string[] }
function emptyMonth() {
  return { month: "2026-09", first_date: "2026-09-01", last_date: "2026-09-30", total: null as any,
    periods: chargingPeriods.map(period => ({ period, ...zeroTotal })), evidence: { ...emptyEvidence },
    days: Array.from({ length: 30 }, (_, index) => ({ local_date: `2026-09-${String(index + 1).padStart(2, "0")}`, future: index > 1,
      total: null as any, periods: chargingPeriods.map(period => ({ period, ...zeroTotal })), evidence: { ...emptyEvidence } })) }
}
function result() {
  return {
    ...scopes[0], state: "held", evidence_boundary: "Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated",
    observed_ac_kwh: "0.500000", eligible_shared_charger_kwh: "0.000000", revision: "results-v2-0123456789abcdef",
    provenance: { source: "synthetic-receiver-kafka", source_epoch: "synthetic-epoch", input_contract: "input-v1", mapping_version: "mapping-v1", manifest_version: "manifest-v1", normalizer_version: "normalizer-v1", calculation_version: "scoped-core-replay-v3", geofence_version: "fictional-geofence-v1" },
    pricing: { state: "held", charger_id: null, rate_models: [], calculation_version: "tou-linear-micro-kwh-v1", estimate_boundary: pricingBoundary, priced_ac_kwh: "0.000000", exact_subtotal: null, display_subtotal: null, quoted_session_count: 0, rate_held_session_count: 0, ineligible_session_count: 1 },
    monthly: { timezone: "America/Los_Angeles", today: "2026-09-02", undated_sessions: 1, evidence: { ...emptyEvidence, held_sessions: 1 }, months: [emptyMonth()] },
    sessions: [{ start_utc_seconds: null as string | null, end_utc_seconds: null as string | null, vehicle: "vehicle-a", connection: "connection-1", classification: "Held", classification_reason: "LocationUnavailable", evidence_state: "complete", observed_ac_kwh: "0.500000", eligible_shared_charger_kwh: "0.000000", quality_flags: [], exclusion_reasons: ["LocationUnavailable"], pricing: { state: "ineligible", hold_reason: "IneligibleSession", quote: null } }],
  }
}

test("persisted client only constructs an explicitly scoped persisted route", () => {
  assert.equal(resultPath(scopes[0]), "/api/poc/owners/owner-a/vehicles/vehicle-a/results")
  assert.equal(resultPath({ owner: "a/b", vehicle: "v?" }), "/api/poc/owners/a%2Fb/vehicles/v%3F/results")
})
test("configured scope discovery accepts live aliases and refuses private or malformed fields", () => {
  const value = { synthetic_only: false, scopes: [{ owner: "fictional-owner", vehicle: "fictional-vehicle" }] }
  assert.deepEqual(parseScopes({ synthetic_only: false, scopes: [] }), { synthetic_only: false, scopes: [] })
  assert.deepEqual(parseScopes(value), value)
  assert.deepEqual(parseScopes({ synthetic_only: true, scopes }), { synthetic_only: true, scopes })
  for (const invalid of [null, {}, { ...value, synthetic_only: "false" },
    { ...value, latitude: 12.3456789 }, { ...value, scopes: [...value.scopes, ...value.scopes] },
    { ...value, scopes: [{ ...value.scopes[0], vin: "FICTIONAL-VIN-A" }] },
    { ...value, scopes: [{ owner: "a/b", vehicle: "fictional-vehicle" }] }]) {
    assert.throws(() => parseScopes(invalid))
  }
})
test("live results retain their counter boundary and exact quotes without synthetic labeling", () => {
  const value = quotedResult()
  value.evidence_boundary = "Vehicle-reported AC input counter energy; physical accuracy unvalidated"
  value.provenance.source = "live-receiver-kafka"
  value.provenance.source_epoch = "live-fictional-charger-v1"
  value.pricing.estimate_boundary = livePricingBoundary
  value.sessions[0].coverage = { connection_start_utc_seconds: "1788264000", connection_end_utc_seconds: "1788264600", first_counter_utc_seconds: "1788264000", last_counter_utc_seconds: "1788264600", usable_readings: 2, partial_start: false, partial_end: false, quantization: liveQuantization }
  const parsed = parseResults(value, scopes[0])
  assert.equal(parsed.pricing.display_subtotal, "$0.11")
  assert.equal(parsed.provenance.source, "live-receiver-kafka")
  value.evidence_boundary = result().evidence_boundary
  assert.throws(() => parseResults(value, scopes[0]))
})
test("scoped energy strings are preserved and another scope is refused", () => {
  assert.equal(parseResults(result(), scopes[0]).observed_ac_kwh, "0.500000")
  assert.throws(() => parseResults(result(), scopes[1]))
  const changed = result()
  changed.sessions[0].vehicle = "vehicle-b"
  assert.throws(() => parseResults(changed, scopes[0]))
})

test("live partial coverage keeps exact zero quotes, quantization and readable UTC times", () => {
  const value: any = result()
  Object.assign(value, { state: "eligible", observed_ac_kwh: "0.000000", eligible_shared_charger_kwh: "0.000000", evidence_boundary: "Vehicle-reported AC input counter energy; physical accuracy unvalidated" })
  value.provenance.source = "live-receiver-kafka"
  Object.assign(value.pricing, { state: "quoted", estimate_boundary: livePricingBoundary, exact_subtotal: "0.0000000000000000 USD", display_subtotal: "$0.00", quoted_session_count: 1, ineligible_session_count: 0 })
  Object.assign(value.sessions[0], { classification: "SharedCharger", classification_reason: "InsideGeofence", evidence_state: "partial", observed_ac_kwh: "0.000000", eligible_shared_charger_kwh: "0.000000", exclusion_reasons: [], quality_flags: ["MissingBaseline", "MissingTerminalSample"],
    pricing: { state: "quoted", hold_reason: null, quote: { exact_cost: "0.0000000000000000 USD", display_cost: "$0.00", slices: [] } },
    start_utc_seconds: "1788263940", end_utc_seconds: "1788264660",
    coverage: { connection_start_utc_seconds: "1788263940", connection_end_utc_seconds: "1788264660", first_counter_utc_seconds: "1788264000", last_counter_utc_seconds: "1788264600", usable_readings: 2, partial_start: true, partial_end: true, quantization: liveQuantization } })
  value.monthly.evidence = { ...emptyEvidence, quoted_sessions: 1, partial_sessions: 1, quality_flags: ["MissingBaseline", "MissingTerminalSample"] }
  value.monthly.undated_sessions = 0
  value.monthly.months[0].evidence = { ...value.monthly.evidence }
  value.monthly.months[0].total = { ...zeroTotal }
  value.monthly.months[0].days[0].evidence = { ...value.monthly.evidence }
  value.monthly.months[0].days[0].total = { ...zeroTotal }
  const parsed = parseResults(value, scopes[0])
  assert.equal(parsed.pricing.display_subtotal, "$0.00")
  assert.match(evidenceLabel(parsed.sessions[0]), /Partial coverage/)
  assert.equal(observationTime("60"), "1970-01-01 00:01:00 UTC")
  assert.equal(observationTime(null), "Unavailable")
  for (const change of [
    (changed: any) => { changed.sessions[0].coverage.usable_readings = 1 },
    (changed: any) => { changed.sessions[0].coverage.first_counter_utc_seconds = null },
    (changed: any) => { changed.sessions[0].coverage.partial_start = false },
    (changed: any) => { changed.sessions[0].coverage.quantization = "Exact physical energy" },
    (changed: any) => { changed.sessions[0].coverage.location = {} },
    (changed: any) => { changed.sessions[0].evidence_state = "complete" },
    (changed: any) => { changed.sessions[0].quality_flags.push("ConflictingEvidence") },
  ]) { const changed = structuredClone(value); change(changed); assert.throws(() => parseResults(changed, scopes[0])) }
})
test("empty and valid zero evidence remain distinct from incomplete held evidence", () => {
  const empty = { ...result(), state: "empty", sessions: [], observed_ac_kwh: "0.000000", pricing: { ...result().pricing, state: "empty", ineligible_session_count: 0 } }
  empty.monthly.evidence = { ...emptyEvidence }
  empty.monthly.undated_sessions = 0
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
    if (classification === "OtherCharger") value.monthly.evidence = { ...emptyEvidence, other_sessions: 1 }
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
  value.pricing = { ...value.pricing, state: "quoted", charger_id: "fictional-shared-charger", rate_models: [{ id: "pge-ev2-a-2026-03-v1", plan: "PGE-EV2-A", source_label: "Public synthetic PG&E EV2-A sample", source_url: "https://www.pge.com/tariffs/assets/pdf/tariffbook/ELEC_SCHEDS_EV2%20%28Sch%29.pdf", effective_from: "2026-03-01", effective_until: null, timezone: "America/Los_Angeles", timezone_rules_version: "2025b", season_rules_version: "june-september-v1", day_type_rules_version: "all-days-v1", holidays: [], periods: chargingPeriods.map((period, index) => ({ season: "Summer", period, start_second: [0, 54000, 57600][index], end_second: [54000, 57600, 75600][index], usd_per_kwh: "0.22558" })) }],
    priced_ac_kwh: "0.500000", exact_subtotal: "0.1127900000000000 USD", display_subtotal: "$0.11", quoted_session_count: 1, ineligible_session_count: 0 }
  Object.assign(value.sessions[0], { classification: "SharedCharger", classification_reason: "InsideGeofence", eligible_shared_charger_kwh: "0.500000", exclusion_reasons: [],
    pricing: { state: "quoted", hold_reason: null, quote: { exact_cost: "0.1127900000000000 USD", display_cost: "$0.11", slices: [{ start_utc_seconds: "1788264000", end_utc_seconds: "1788264600", local_date: "2026-09-01", local_start: "2026-09-01T05:00:00-07:00", local_end: "2026-09-01T05:10:00-07:00", utc_offset_seconds: "-25200", season: "Summer", day_type: "Weekday", period: "off_peak", model_version: "pge-ev2-a-2026-03-v1", allocated_ac_kwh: "0.500000", usd_per_kwh: "0.2255800000 USD/kWh", exact_cost: "0.1127900000000000 USD" }] } } })
  value.sessions[0].start_utc_seconds = "1788264000"
  value.sessions[0].end_utc_seconds = "1788264600"
  value.monthly.undated_sessions = 0
  value.monthly.evidence = { ...emptyEvidence, quoted_sessions: 1 }
  const total = { ac_kwh: "0.500000", exact_cost: "0.1127900000000000 USD", display_cost: "$0.11" }
  const month = value.monthly.months[0]
  month.total = total
  month.periods[0] = { period: "off_peak", ...total }
  month.evidence = { ...emptyEvidence, quoted_sessions: 1 }
  month.days[0] = { ...month.days[0], total, periods: structuredClone(month.periods), evidence: { ...emptyEvidence, quoted_sessions: 1 } }
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
  value.monthly.evidence = { ...emptyEvidence, held_sessions: 1 }
  value.monthly.months[0] = emptyMonth()
  value.monthly.months[0].evidence = { ...emptyEvidence, held_sessions: 1 }
  value.monthly.months[0].days[0].evidence = { ...emptyEvidence, held_sessions: 1 }
  assert.equal(parseResults(value, scopes[0]).sessions[0].pricing.quote, null)
  assert.match(pricingHoldLabel("MissingRate"), /no selected rate coverage/)
  const zero = quotedResult()
  Object.assign(zero, { observed_ac_kwh: "0.000000", eligible_shared_charger_kwh: "0.000000" })
  Object.assign(zero.sessions[0], { observed_ac_kwh: "0.000000", eligible_shared_charger_kwh: "0.000000", evidence_state: "complete_zero" })
  zero.sessions[0].pricing.quote = { exact_cost: "0.0000000000000000 USD", display_cost: "$0.00", slices: [] }
  Object.assign(zero.pricing, { priced_ac_kwh: "0.000000", exact_subtotal: "0.0000000000000000 USD", display_subtotal: "$0.00", rate_models: [] })
  zero.monthly.evidence = { ...emptyEvidence, quoted_sessions: 1, complete_zero_sessions: 1 }
  const month = zero.monthly.months[0]
  month.total = { ...zeroTotal }
  month.periods = chargingPeriods.map(period => ({ period, ...zeroTotal }))
  month.evidence = { ...zero.monthly.evidence }
  month.days[0] = { ...month.days[0], total: { ...zeroTotal }, periods: structuredClone(month.periods), evidence: { ...month.evidence } }
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

test("monthly history preserves exact totals, empty days and matching accessible day details", () => {
  const value = parseResults(quotedResult(), scopes[0])
  const month = value.monthly.months[0]
  assert.equal(month.days.length, 30)
  assert.deepEqual(month.total, { ac_kwh: "0.500000", exact_cost: "0.1127900000000000 USD", display_cost: "$0.11" })
  assert.deepEqual(month.periods.map(period => period.period), ["off_peak", "part_peak", "peak"])
  assert.match(dayLabel(month.days[0]), /0.500000 kWh, estimated charging cost \$0.11/)
  assert.equal(month.days[1].total, null)
  assert.match(dayLabel(month.days[2]), /No quoted charging · Future day/)
  assert.equal(value.sessions[0].start_utc_seconds, "1788264000")
})

test("monthly history rejects unsafe fields, malformed calendars and invented held totals", () => {
  for (const change of [
    (value: any) => { delete value.monthly },
    (value: any) => { value.revision = "results-v1-0123456789abcdef" },
    (value: any) => { value.monthly.latitude = 0 },
    (value: any) => { value.monthly.timezone = "UTC" },
    (value: any) => { value.monthly.today = "2026-09-31" },
    (value: any) => { value.monthly.undated_sessions = 2 },
    (value: any) => { value.monthly.evidence.held_sessions = 1 },
    (value: any) => { value.monthly.evidence.quality_flags = ["PrivateDiagnostics"] },
    (value: any) => { value.monthly.months.push(structuredClone(value.monthly.months[0])) },
    (value: any) => { value.monthly.months[0].last_date = "2026-09-29" },
    (value: any) => { value.monthly.months[0].days.pop() },
    (value: any) => { value.monthly.months[0].days[0].local_date = "2026-09-02" },
    (value: any) => { value.monthly.months[0].days[0].future = true },
    (value: any) => { value.monthly.months[0].periods.reverse() },
    (value: any) => { value.monthly.months[0].total.exact_cost = 0.11279 },
    (value: any) => { value.monthly.months[0].days[1].total = { ...zeroTotal } },
    (value: any) => { value.monthly.months[0].days[1].periods[0].ac_kwh = "1.000000" },
    (value: any) => { value.sessions[0].start_utc_seconds = 1788264000 },
    (value: any) => { value.sessions[0].end_utc_seconds = "Invalid" },
  ]) {
    const value = quotedResult()
    change(value)
    assert.throws(() => parseResults(value, scopes[0]))
  }
})

test("registry discovery accepts display names and empty active scope without private fields", () => {
  const value = { synthetic_only: false, scopes: [{ owner: "owner-b", vehicle: "vehicle-b", name: "Model Y sample", chargers: [{ id: "driveway", name: "Driveway charger" }, { id: "garage", name: "Garage charger" }] }] }
  assert.deepEqual(parseScopes(value),value)
  assert.deepEqual(parseScopes({ synthetic_only: false, scopes: [] }), { synthetic_only: false, scopes: [] })
  assert.equal(resultPath(value.scopes[0],"garage"),"/api/poc/owners/owner-b/vehicles/vehicle-b/results?charger=garage")
  for (const change of [
    (value: any) => { value.scopes[0].name = "  " },
    (value: any) => { value.scopes[0].chargers[0].latitude = 12.3456789 },
    (value: any) => { value.scopes[0].chargers[0].name = "" },
    (value: any) => { value.scopes[0].chargers.push(value.scopes[0].chargers[0]) },
  ]) { const changed=structuredClone(value);change(changed);assert.throws(()=>parseScopes(changed)) }
})

test("named registry result retains session charger and immutable version provenance", () => {
  const value: any=quotedResult()
  value.registry={car_name:"Model 3 sample",chargers:[{id:"driveway",name:"Driveway charger"}],selected_charger:null}
  value.pricing.charger_id=null
  value.sessions[0].charger={id:"driveway",name:"Driveway charger",assignment_versions:["assignment-v1"],geofence_versions:["geofence-v1"]}
  assert.equal(parseResults(value,scopes[0]).registry?.car_name,"Model 3 sample")
  assert.equal(parseResults(value,scopes[0]).sessions[0].charger?.name,"Driveway charger")
  for (const change of [
    (value: any) => { value.registry.car_name = "  " },
    (value: any) => { value.registry.selected_charger = "unknown" },
    (value: any) => { value.sessions[0].charger.id = "unknown" },
    (value: any) => { value.sessions[0].charger.geofence_versions = [] },
    (value: any) => { value.sessions[0].charger.latitude = 12.3456789 },
  ]) { const changed=structuredClone(value);change(changed);assert.throws(()=>parseResults(changed,scopes[0])) }
})
