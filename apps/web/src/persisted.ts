export type Scope = { owner: string; vehicle: string }
export type RateModel = {
  id: string; plan: string; source_label: string; source_url: string
  effective_from: string; effective_until: string | null
  timezone: string; timezone_rules_version: string; season_rules_version: string; day_type_rules_version: string
  holidays: string[]
  periods: { season: string; period: string; start_second: number; end_second: number; usd_per_kwh: string }[]
}
export type RatedSlice = {
  start_utc_seconds: string; end_utc_seconds: string; local_date: string; local_start: string; local_end: string; utc_offset_seconds: string
  season: string; day_type: string; period: string; model_version: string
  allocated_ac_kwh: string; usd_per_kwh: string; exact_cost: string
}
export type SessionPrice = {
  state: "quoted" | "rate_held" | "ineligible"
  hold_reason: string | null
  quote: { exact_cost: string; display_cost: string; slices: RatedSlice[] } | null
}
export type PricingResult = {
  state: "empty" | "quoted" | "held"; charger_id: string | null; rate_models: RateModel[]
  calculation_version: string; estimate_boundary: string; priced_ac_kwh: string
  exact_subtotal: string | null; display_subtotal: string | null
  quoted_session_count: number; rate_held_session_count: number; ineligible_session_count: number
}
export const pricingBoundary = "Synthetic PG&E bundled energy-only estimate; counter energy allocated by elapsed UTC time; excludes CCA/PCIA, baseline credit, daily Base Services Charge and taxes; not a bill"
export type SessionResult = {
  vehicle: string
  connection: string
  classification: "SharedCharger" | "OtherCharger" | "Held"
  classification_reason: "InsideGeofence" | "OutsideGeofence" | "LocationUnavailable" | "LocationAmbiguous"
  evidence_state: "complete" | "complete_zero" | "incomplete"
  observed_ac_kwh: string
  eligible_shared_charger_kwh: string
  quality_flags: string[]
  exclusion_reasons: string[]
  pricing: SessionPrice
}
export type ScopedResults = Scope & {
  state: "empty" | "held" | "eligible"
  evidence_boundary: string
  observed_ac_kwh: string
  eligible_shared_charger_kwh: string
  sessions: SessionResult[]
  pricing: PricingResult
  revision: string
  provenance: {
    source: string
    source_epoch: string
    input_contract: string
    mapping_version: string
    manifest_version: string
    normalizer_version: string
    calculation_version: string
    geofence_version: string
  }
}
export const scopes: Scope[] = [
  { owner: "owner-a", vehicle: "vehicle-a" },
  { owner: "owner-b", vehicle: "vehicle-b" },
]
const qualityReasons = ["MissingConnectionStart", "MissingConnectionEnd", "MissingBaseline", "MissingTerminalSample", "CounterRollback", "ConflictingEvidence", "InvalidBoundaryOrder", "InvalidCounter(Invalid)", "InvalidCounter(Negative)", "InvalidCounter(UnsupportedPrecision)", "InvalidCounter(Overflow)"]
const exclusionReasons = ["LocationUnavailable", "LocationAmbiguous", "OtherCharger", "DcEvidence", "AmbiguousChargeType"]
const classificationReasons = { SharedCharger: ["InsideGeofence"], OtherCharger: ["OutsideGeofence"], Held: ["LocationUnavailable", "LocationAmbiguous"] }
const resultKeys = ["owner", "vehicle", "state", "evidence_boundary", "observed_ac_kwh", "eligible_shared_charger_kwh", "sessions", "revision", "provenance", "pricing"]
const sessionKeys = ["vehicle", "connection", "classification", "classification_reason", "evidence_state", "observed_ac_kwh", "eligible_shared_charger_kwh", "quality_flags", "exclusion_reasons", "pricing"]
const provenanceKeys = ["source", "source_epoch", "input_contract", "mapping_version", "manifest_version", "normalizer_version", "calculation_version", "geofence_version"]
function hasExactKeys(value: unknown, keys: string[]) {
  return !!value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).length === keys.length && Object.keys(value).every(key => keys.includes(key))
}
function isEligible(session: SessionResult) {
  return session.classification === "SharedCharger" && !session.quality_flags.length && !session.exclusion_reasons.length
}
export function resultPath(scope: Scope) {
  return `/api/poc/owners/${encodeURIComponent(scope.owner)}/vehicles/${encodeURIComponent(scope.vehicle)}/results`
}
export function parseScopes(value: unknown): { synthetic_only: boolean; scopes: Scope[] } {
  const object = value as { synthetic_only: boolean; scopes: Scope[] }
  if (!hasExactKeys(object, ["synthetic_only", "scopes"]) || typeof object.synthetic_only !== "boolean" ||
      !Array.isArray(object.scopes) || !object.scopes.length || object.scopes.length > 256 ||
      object.scopes.some(scope => !hasExactKeys(scope, ["owner", "vehicle"]) || !alias(scope.owner) || !alias(scope.vehicle)) ||
      new Set(object.scopes.map(scope => scope.vehicle)).size !== object.scopes.length) {
    throw new Error("Persisted scopes unavailable")
  }
  return object
}
export function parseResults(value: unknown, scope: Scope): ScopedResults {
  const object = value as ScopedResults
  const energy = (value: unknown) => typeof value === "string" && /^\d+\.\d{6}$/.test(value)
  const reasons = (value: unknown, allowed: string[]) => Array.isArray(value) && value.every(reason => allowed.includes(reason))
  if (!hasExactKeys(object, resultKeys) || object.owner !== scope.owner || object.vehicle !== scope.vehicle ||
      !["empty", "held", "eligible"].includes(object.state) || !energy(object.observed_ac_kwh) || !energy(object.eligible_shared_charger_kwh) ||
      !["Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated", "Vehicle-reported AC input counter energy; physical accuracy unvalidated"].includes(object.evidence_boundary) ||
      typeof object.revision !== "string" || !/^results-v1-[a-f0-9]{16}$/.test(object.revision) ||
      !Array.isArray(object.sessions) || (object.state === "empty") !== (object.sessions.length === 0) ||
      !hasExactKeys(object.provenance, provenanceKeys) || !["synthetic-receiver-kafka", "live-receiver-kafka"].includes(object.provenance.source) ||
      object.provenance.calculation_version !== "scoped-core-replay-v2" ||
      !provenanceKeys.every(key => typeof object.provenance[key as keyof ScopedResults["provenance"]] === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(object.provenance[key as keyof ScopedResults["provenance"]])) ||
      object.sessions.some(session => !hasExactKeys(session, sessionKeys) || session.vehicle !== scope.vehicle ||
        !/^[A-Za-z0-9_-]{1,64}$/.test(session.connection) ||
        !Object.hasOwn(classificationReasons, session.classification) ||
        !classificationReasons[session.classification].includes(session.classification_reason) ||
        !["complete", "complete_zero", "incomplete"].includes(session.evidence_state) ||
        !energy(session.observed_ac_kwh) || !energy(session.eligible_shared_charger_kwh) ||
        !reasons(session.quality_flags, qualityReasons) || !reasons(session.exclusion_reasons, exclusionReasons) ||
        session.eligible_shared_charger_kwh !== (isEligible(session) ? session.observed_ac_kwh : "0.000000") ||
        (session.classification === "Held" && !session.exclusion_reasons.includes(session.classification_reason)) ||
        (session.classification === "OtherCharger" && !session.exclusion_reasons.includes("OtherCharger")))) {
    throw new Error("Persisted results unavailable")
  }
  if (object.sessions.length && (object.state === "eligible") !== object.sessions.every(isEligible)) throw new Error("Persisted results unavailable")
  if (!object.sessions.some(isEligible) && object.eligible_shared_charger_kwh !== "0.000000") throw new Error("Persisted results unavailable")
  if ((object.provenance.source === "live-receiver-kafka") !==
      (object.evidence_boundary === "Vehicle-reported AC input counter energy; physical accuracy unvalidated")) throw new Error("Persisted results unavailable")
  validatePricing(object)
  return object
}
export function evidenceLabel(session: SessionResult) {
  if (session.evidence_state === "incomplete") return "Incomplete evidence"
  if (session.evidence_state === "complete_zero") return "Complete zero-energy evidence"
  return "Complete counter evidence"
}
export function classificationLabel(session: SessionResult) {
  return `${session.classification} · ${isEligible(session) ? "Eligible" : session.classification === "OtherCharger" ? "Excluded" : "Held"}`
}
export function reasonLabel(reason: string) {
  if (reason === "InsideGeofence") return "All session locations are inside the shared charger geofence."
  if (reason === "OutsideGeofence") return "All session locations are outside the shared charger geofence."
  if (reason === "LocationUnavailable") return "Held automatically: session location is unavailable or invalid."
  if (reason === "LocationAmbiguous") return "Held automatically: session location is partial, mixed or conflicting."
  return reason.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/[()]/g, " ").trim()
}

const alias = (value: unknown) => typeof value === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(value)
const date = (value: unknown) => typeof value === "string" && /^\d{4}-\d{2}-\d{2}$/.test(value)
const localTime = (value: unknown) => typeof value === "string" && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}$/.test(value)
const energyString = (value: unknown) => typeof value === "string" && /^\d+\.\d{6}$/.test(value)
const exactCost = (value: unknown) => typeof value === "string" && /^\d+\.\d{16} USD$/.test(value)
const displayCost = (value: unknown) => typeof value === "string" && /^\$\d+\.\d{2}$/.test(value)
const decimalRate = (value: unknown) => typeof value === "string" && /^\d+(\.\d{1,10})?$/.test(value)
function validatePricing(results: ScopedResults) {
  const pricing = results.pricing
  const refuse = () => { throw new Error("Persisted results unavailable") }
  if (!hasExactKeys(pricing, ["state", "charger_id", "rate_models", "calculation_version", "estimate_boundary", "priced_ac_kwh", "exact_subtotal", "display_subtotal", "quoted_session_count", "rate_held_session_count", "ineligible_session_count"]) ||
      !["empty", "quoted", "held"].includes(pricing.state) || (pricing.charger_id !== null && !alias(pricing.charger_id)) ||
      pricing.calculation_version !== "tou-linear-micro-kwh-v1" || pricing.estimate_boundary !== pricingBoundary || !energyString(pricing.priced_ac_kwh) ||
      !Array.isArray(pricing.rate_models) || (pricing.charger_id === null && pricing.rate_models.length)) refuse()
  for (const model of pricing.rate_models) {
    if (!hasExactKeys(model, ["id", "plan", "source_label", "source_url", "effective_from", "effective_until", "timezone", "timezone_rules_version", "season_rules_version", "day_type_rules_version", "holidays", "periods"]) ||
        !alias(model.id) || !alias(model.plan) || typeof model.source_label !== "string" || !/^[A-Za-z0-9 &_()./-]{1,160}$/.test(model.source_label) ||
        model.source_url !== "https://www.pge.com/tariffs/assets/pdf/tariffbook/ELEC_SCHEDS_EV2%20%28Sch%29.pdf" ||
        !date(model.effective_from) || (model.effective_until !== null && !date(model.effective_until)) || model.timezone !== "America/Los_Angeles" ||
        !/^\d{4}[a-z]$/.test(model.timezone_rules_version) || model.season_rules_version !== "june-september-v1" || model.day_type_rules_version !== "all-days-v1" ||
        !Array.isArray(model.holidays) || !model.holidays.every(date) || !Array.isArray(model.periods)) refuse()
    for (const period of model.periods) {
      if (!hasExactKeys(period, ["season", "period", "start_second", "end_second", "usd_per_kwh"]) || !["Summer", "Winter"].includes(period.season) || !alias(period.period) ||
          !Number.isInteger(period.start_second) || !Number.isInteger(period.end_second) || period.start_second < 0 || period.end_second > 86400 || period.start_second >= period.end_second || !decimalRate(period.usd_per_kwh)) refuse()
    }
  }
  for (const session of results.sessions) {
    const price = session.pricing
    if (!hasExactKeys(price, ["state", "hold_reason", "quote"]) || !["quoted", "rate_held", "ineligible"].includes(price.state) ||
        (price.state === "ineligible") === isEligible(session)) refuse()
    if (price.state !== "quoted") {
      if (price.quote !== null || !(price.state === "ineligible" ? price.hold_reason === "IneligibleSession" : ["MissingRate", "UnresolvedRateBoundary", "NonIncreasingTime"].includes(price.hold_reason ?? ""))) refuse()
      continue
    }
    const quote = price.quote
    if (price.hold_reason !== null || !hasExactKeys(quote, ["exact_cost", "display_cost", "slices"]) || !quote || !exactCost(quote.exact_cost) || !displayCost(quote.display_cost) || !Array.isArray(quote.slices)) refuse()
    if (!quote) refuse()
    if (session.observed_ac_kwh === "0.000000" && (quote!.exact_cost !== "0.0000000000000000 USD" || quote!.display_cost !== "$0.00" || quote!.slices.length)) refuse()
    if (session.observed_ac_kwh !== "0.000000" && !quote!.slices.length) refuse()
    for (const slice of quote!.slices) {
      if (!hasExactKeys(slice, ["start_utc_seconds", "end_utc_seconds", "local_date", "local_start", "local_end", "utc_offset_seconds", "season", "day_type", "period", "model_version", "allocated_ac_kwh", "usd_per_kwh", "exact_cost"]) ||
          !/^-?\d+$/.test(slice.start_utc_seconds) || !/^-?\d+$/.test(slice.end_utc_seconds) || !/^-?\d+$/.test(slice.utc_offset_seconds) ||
          !date(slice.local_date) || !localTime(slice.local_start) || !localTime(slice.local_end) || !["Summer", "Winter"].includes(slice.season) || !["Weekday", "Weekend", "Holiday"].includes(slice.day_type) || !alias(slice.period) || !alias(slice.model_version) ||
          !energyString(slice.allocated_ac_kwh) || !/^\d+\.\d{10} USD\/kWh$/.test(slice.usd_per_kwh) || !exactCost(slice.exact_cost) ||
          !pricing.rate_models.some(model => model.id === slice.model_version && model.periods.some(period => period.season === slice.season && period.period === slice.period))) refuse()
    }
  }
  for (const [state, count] of [["quoted", pricing.quoted_session_count], ["rate_held", pricing.rate_held_session_count], ["ineligible", pricing.ineligible_session_count]] as const) {
    if (!Number.isInteger(count) || count !== results.sessions.filter(session => session.pricing.state === state).length) refuse()
  }
  const expectedState = !results.sessions.length ? "empty" : results.sessions.every(session => session.pricing.state === "quoted") ? "quoted" : "held"
  if (pricing.state !== expectedState || (pricing.quoted_session_count ? !exactCost(pricing.exact_subtotal) || !displayCost(pricing.display_subtotal) : pricing.exact_subtotal !== null || pricing.display_subtotal !== null || pricing.priced_ac_kwh !== "0.000000")) refuse()
}
export function pricingHoldLabel(reason: string | null) {
  if (reason === "MissingRate") return "Rate held: an observed interval has no selected rate coverage."
  if (reason === "UnresolvedRateBoundary") return "Rate held: a time boundary could not be resolved."
  if (reason === "NonIncreasingTime") return "Rate held: counter timestamps do not increase."
  return "No quote: automatic classification or evidence quality excludes this session."
}
