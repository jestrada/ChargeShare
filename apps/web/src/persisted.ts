export type ChargerOption = { id: string; name: string }
export type Scope = { owner: string; vehicle: string; name?: string; chargers?: ChargerOption[] }
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
export const livePricingBoundary = "Public sample-rate PG&E bundled energy-only estimate; observed-window counter energy allocated by elapsed UTC time; excludes CCA/PCIA, baseline credit, daily Base Services Charge and taxes; not a bill"
export const liveQuantization = "Live counters quantized once to 0.000001 kWh, decimal half-up"
export type CounterCoverage = {
  connection_start_utc_seconds: string | null; connection_end_utc_seconds: string | null
  first_counter_utc_seconds: string | null; last_counter_utc_seconds: string | null
  usable_readings: number; partial_start: boolean; partial_end: boolean; quantization: string
}
export type SessionResult = {
  vehicle: string
  connection: string
  start_utc_seconds: string | null
  end_utc_seconds: string | null
  classification: "SharedCharger" | "OtherCharger" | "Held"
  classification_reason: "InsideGeofence" | "OutsideGeofence" | "LocationUnavailable" | "LocationAmbiguous"
  evidence_state: "complete" | "complete_zero" | "incomplete" | "partial"
  observed_ac_kwh: string
  eligible_shared_charger_kwh: string
  quality_flags: string[]
  exclusion_reasons: string[]
  pricing: SessionPrice
  charger?: ChargerOption & { assignment_versions: string[]; geofence_versions: string[] }
  location_versions?: { charger_id: string; assignment_version: string; geofence_version: string }[]
  coverage?: CounterCoverage
}
export type ScopedResults = Scope & {
  state: "empty" | "held" | "eligible"
  evidence_boundary: string
  observed_ac_kwh: string
  eligible_shared_charger_kwh: string
  sessions: SessionResult[]
  pricing: PricingResult
  monthly: MonthlyHistory
  registry?: { car_name: string; chargers: ChargerOption[]; selected_charger: string | null }
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
export const chargingPeriods = ["off_peak", "part_peak", "peak"] as const
export type ChargingPeriod = typeof chargingPeriods[number]
export type ChargingTotal = { ac_kwh: string; exact_cost: string; display_cost: string }
export type PeriodTotal = ChargingTotal & { period: ChargingPeriod }
export type ChargingEvidence = {
  quoted_sessions: number; held_sessions: number; other_sessions: number
  partial_sessions: number; complete_zero_sessions: number; quality_flags: string[]
}
export type ChargingDay = {
  local_date: string; future: boolean; periods: PeriodTotal[]
  total: ChargingTotal | null; evidence: ChargingEvidence
}
export type ChargingMonth = {
  month: string; first_date: string; last_date: string; days: ChargingDay[]
  periods: PeriodTotal[]; total: ChargingTotal | null; evidence: ChargingEvidence
}
export type MonthlyHistory = {
  timezone: string; today: string; months: ChargingMonth[]
  evidence: ChargingEvidence; undated_sessions: number
}
export const scopes: Scope[] = [
  { owner: "owner-a", vehicle: "vehicle-a" },
  { owner: "owner-b", vehicle: "vehicle-b" },
]
const qualityReasons = ["InsufficientCounterReadings", "MissingConnectionStart", "MissingConnectionEnd", "MissingBaseline", "MissingTerminalSample", "CounterRollback", "ConflictingEvidence", "InvalidBoundaryOrder", "InvalidCounter(Invalid)", "InvalidCounter(Negative)", "InvalidCounter(UnsupportedPrecision)", "InvalidCounter(Overflow)"]
const exclusionReasons = ["LocationUnavailable", "LocationAmbiguous", "OtherCharger", "DcEvidence", "AmbiguousChargeType"]
const classificationReasons = { SharedCharger: ["InsideGeofence"], OtherCharger: ["OutsideGeofence"], Held: ["LocationUnavailable", "LocationAmbiguous"] }
const resultKeys = ["owner", "vehicle", "state", "evidence_boundary", "observed_ac_kwh", "eligible_shared_charger_kwh", "sessions", "revision", "provenance", "pricing", "monthly"]
const sessionKeys = ["vehicle", "connection", "start_utc_seconds", "end_utc_seconds", "classification", "classification_reason", "evidence_state", "observed_ac_kwh", "eligible_shared_charger_kwh", "quality_flags", "exclusion_reasons", "pricing"]
const provenanceKeys = ["source", "source_epoch", "input_contract", "mapping_version", "manifest_version", "normalizer_version", "calculation_version", "geofence_version"]
function hasExactKeys(value: unknown, keys: string[]) {
  return !!value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).length === keys.length && Object.keys(value).every(key => keys.includes(key))
}
function isEligible(session: SessionResult) {
  return session.classification === "SharedCharger" && !session.exclusion_reasons.length &&
    session.quality_flags.every(flag => !!session.coverage && session.coverage.usable_readings >= 2 && ["MissingBaseline", "MissingTerminalSample"].includes(flag))
}
export function resultPath(scope: Scope, charger?: string) {
  return `/api/poc/owners/${encodeURIComponent(scope.owner)}/vehicles/${encodeURIComponent(scope.vehicle)}/results${charger ? `?charger=${encodeURIComponent(charger)}` : ""}`
}
export function parseScopes(value: unknown): { synthetic_only: boolean; scopes: Scope[] } {
  const object = value as { synthetic_only: boolean; scopes: Scope[] }
  if (!hasExactKeys(object, ["synthetic_only", "scopes"]) || typeof object.synthetic_only !== "boolean" ||
      !Array.isArray(object.scopes) || object.scopes.length > 256 ||
      object.scopes.some(scope => !hasExactKeys(scope, scope.name === undefined ? ["owner", "vehicle"] : ["owner", "vehicle", "name", "chargers"]) || !alias(scope.owner) || !alias(scope.vehicle) || (scope.name !== undefined && (!displayName(scope.name) || !chargerOptions(scope.chargers)))) ||
      new Set(object.scopes.map(scope => scope.vehicle)).size !== object.scopes.length) {
    throw new Error("Persisted scopes unavailable")
  }
  return object
}
export function parseResults(value: unknown, scope: Scope): ScopedResults {
  const object = value as ScopedResults
  const energy = (value: unknown) => typeof value === "string" && /^\d+\.\d{6}$/.test(value)
  const reasons = (value: unknown, allowed: string[]) => Array.isArray(value) && value.every(reason => allowed.includes(reason))
  if (!hasExactKeys(object, object.registry ? [...resultKeys, "registry"] : resultKeys) || object.owner !== scope.owner || object.vehicle !== scope.vehicle ||
      !["empty", "held", "eligible"].includes(object.state) || !energy(object.observed_ac_kwh) || !energy(object.eligible_shared_charger_kwh) ||
      !["Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated", "Vehicle-reported AC input counter energy; physical accuracy unvalidated"].includes(object.evidence_boundary) ||
      typeof object.revision !== "string" || !/^results-v2-[a-f0-9]{16}$/.test(object.revision) ||
      !Array.isArray(object.sessions) || (object.state === "empty") !== (object.sessions.length === 0) ||
      !hasExactKeys(object.provenance, provenanceKeys) || !["synthetic-receiver-kafka", "live-receiver-kafka"].includes(object.provenance.source) ||
      object.provenance.calculation_version !== "scoped-core-replay-v3" ||
      !provenanceKeys.every(key => typeof object.provenance[key as keyof ScopedResults["provenance"]] === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(object.provenance[key as keyof ScopedResults["provenance"]])) ||
      object.sessions.some(session => !hasExactKeys(session, [...sessionKeys, ...(object.provenance.source === "live-receiver-kafka" ? ["coverage"] : []), ...(session.charger ? ["charger"] : []), ...(session.location_versions ? ["location_versions"] : [])]) || !validCoverage(session, object.provenance.source === "live-receiver-kafka") || session.vehicle !== scope.vehicle ||
        ![session.start_utc_seconds, session.end_utc_seconds].every(epochSeconds) ||
        !/^[A-Za-z0-9_-]{1,64}$/.test(session.connection) ||
        !Object.hasOwn(classificationReasons, session.classification) ||
        !classificationReasons[session.classification].includes(session.classification_reason) ||
        !["complete", "complete_zero", "incomplete", "partial"].includes(session.evidence_state) ||
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
  if (object.registry && (!hasExactKeys(object.registry, ["car_name", "chargers", "selected_charger"]) ||
      !displayName(object.registry.car_name) || !chargerOptions(object.registry.chargers) ||
      object.registry.selected_charger !== null && !object.registry.chargers.some(charger => charger.id === object.registry!.selected_charger) ||
      object.sessions.some(session => session.charger && (!hasExactKeys(session.charger, ["id", "name", "assignment_versions", "geofence_versions"]) ||
        !alias(session.charger.id) || !displayName(session.charger.name) || !object.registry!.chargers.some(charger => charger.id === session.charger!.id && charger.name === session.charger!.name) ||
        ![session.charger.assignment_versions,session.charger.geofence_versions].every(versions => Array.isArray(versions) && versions.length > 0 && versions.every(alias)))))) {
    throw new Error("Persisted results unavailable")
  }
  if (object.sessions.some(session => session.location_versions && (!Array.isArray(session.location_versions) || session.location_versions.some(version =>
    !hasExactKeys(version,["charger_id","assignment_version","geofence_version"]) || !alias(version.charger_id) || !alias(version.assignment_version) || !alias(version.geofence_version))))) {
    throw new Error("Persisted results unavailable")
  }
  validatePricing(object)
  validateMonthly(object)
  return object
}
function validCoverage(session: SessionResult, live: boolean) {
  if (!live) return !session.coverage && session.evidence_state !== "partial"
  const coverage = session.coverage
  if (!coverage || !hasExactKeys(coverage, ["connection_start_utc_seconds", "connection_end_utc_seconds", "first_counter_utc_seconds", "last_counter_utc_seconds", "usable_readings", "partial_start", "partial_end", "quantization"]) ||
      !Number.isSafeInteger(coverage.usable_readings) || coverage.usable_readings < 0 || coverage.quantization !== liveQuantization ||
      typeof coverage.partial_start !== "boolean" || typeof coverage.partial_end !== "boolean" || !Array.isArray(session.quality_flags) ||
      coverage.partial_start !== session.quality_flags.includes("MissingBaseline") || coverage.partial_end !== session.quality_flags.includes("MissingTerminalSample")) return false
  if ((coverage.usable_readings < 2) !== session.quality_flags.includes("InsufficientCounterReadings") ||
      (coverage.connection_start_utc_seconds === null) !== session.quality_flags.includes("MissingConnectionStart") ||
      (coverage.connection_end_utc_seconds === null) !== session.quality_flags.includes("MissingConnectionEnd")) return false
  const times = [coverage.connection_start_utc_seconds, coverage.connection_end_utc_seconds, coverage.first_counter_utc_seconds, coverage.last_counter_utc_seconds]
  if (!times.every(time => time === null || (typeof time === "string" && /^-?\d+$/.test(time)))) return false
  if ((coverage.usable_readings === 0) !== (coverage.first_counter_utc_seconds === null && coverage.last_counter_utc_seconds === null)) return false
  if (coverage.usable_readings > 0 && (coverage.first_counter_utc_seconds === null || coverage.last_counter_utc_seconds === null || BigInt(coverage.first_counter_utc_seconds) > BigInt(coverage.last_counter_utc_seconds))) return false
  if ((session.evidence_state === "partial") !== (isEligible(session) && (coverage.partial_start || coverage.partial_end))) return false
  return true
}
export function observationTime(seconds: string | null) {
  if (seconds === null) return "Unavailable"
  const time = new Date(Number(seconds) * 1000)
  return Number.isFinite(time.getTime()) ? time.toISOString().replace("T", " ").replace(".000Z", " UTC") : "Unavailable"
}
export function evidenceLabel(session: SessionResult) {
  if (session.evidence_state === "partial") return "Partial coverage · observed counter window"
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

const displayName = (value: unknown) => typeof value === "string" && value.trim().length > 0 && value.length <= 128 && !/[\u0000-\u001f\u007f]/.test(value)
const chargerOptions = (value: unknown): value is ChargerOption[] => Array.isArray(value) && value.length <= 256 &&
  value.every(charger => hasExactKeys(charger,["id","name"]) && alias(charger.id) && displayName(charger.name)) && new Set(value.map(charger => charger.id)).size === value.length
const alias = (value: unknown) => typeof value === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(value)
const date = (value: unknown) => typeof value === "string" && /^\d{4}-\d{2}-\d{2}$/.test(value) &&
  Number.isFinite(Date.parse(`${value}T00:00:00Z`)) && new Date(`${value}T00:00:00Z`).toISOString().slice(0, 10) === value
const epochSeconds = (value: unknown) => value === null || typeof value === "string" && /^-?\d+$/.test(value)
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
      pricing.calculation_version !== "tou-linear-micro-kwh-v1" || pricing.estimate_boundary !== (results.provenance.source === "live-receiver-kafka" ? livePricingBoundary : pricingBoundary) || !energyString(pricing.priced_ac_kwh) ||
      !Array.isArray(pricing.rate_models) || (pricing.charger_id === null && pricing.rate_models.length && !results.registry)) refuse()
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

function validateMonthly(results: ScopedResults) {
  const refuse = () => { throw new Error("Persisted results unavailable") }
  const history = results.monthly
  const counts = ["quoted_sessions", "held_sessions", "other_sessions", "partial_sessions", "complete_zero_sessions"] as const
  function validEvidence(evidence: ChargingEvidence) {
    return hasExactKeys(evidence, [...counts, "quality_flags"]) &&
      counts.every(key => Number.isSafeInteger(evidence[key]) && evidence[key] >= 0 && evidence[key] <= results.sessions.length) &&
      evidence.partial_sessions + evidence.complete_zero_sessions <= evidence.quoted_sessions &&
      Array.isArray(evidence.quality_flags) && evidence.quality_flags.every(flag => qualityReasons.includes(flag)) &&
      new Set(evidence.quality_flags).size === evidence.quality_flags.length
  }
  function validTotal(total: ChargingTotal, period = false) {
    return hasExactKeys(total, period ? ["period", "ac_kwh", "exact_cost", "display_cost"] : ["ac_kwh", "exact_cost", "display_cost"]) &&
      energyString(total.ac_kwh) && exactCost(total.exact_cost) && displayCost(total.display_cost)
  }
  function validSummary(summary: { total: ChargingTotal | null; periods: PeriodTotal[]; evidence: ChargingEvidence }) {
    return validEvidence(summary.evidence) && (summary.total === null ? summary.evidence.quoted_sessions === 0 : summary.evidence.quoted_sessions > 0 && validTotal(summary.total)) &&
      Array.isArray(summary.periods) && summary.periods.length === 3 &&
      summary.periods.every((period, index) => validTotal(period, true) && period.period === chargingPeriods[index] &&
        (summary.total !== null || period.ac_kwh === "0.000000" && period.exact_cost === "0.0000000000000000 USD" && period.display_cost === "$0.00"))
  }
  if (!hasExactKeys(history, ["timezone", "today", "months", "evidence", "undated_sessions"]) ||
      history.timezone !== "America/Los_Angeles" || !date(history.today) || !validEvidence(history.evidence) ||
      !Number.isSafeInteger(history.undated_sessions) || history.undated_sessions < 0 || history.undated_sessions > results.sessions.length ||
      !Array.isArray(history.months) || !history.months.length || history.months[0]?.month !== history.today.slice(0, 7)) refuse()
  const evidence = history.evidence
  const undatedSessions = results.sessions.filter(session => session.start_utc_seconds === null && session.end_utc_seconds === null &&
    !session.coverage?.first_counter_utc_seconds && !session.coverage?.last_counter_utc_seconds && !session.pricing.quote?.slices.length).length
  if (evidence.quoted_sessions !== results.pricing.quoted_session_count ||
      history.undated_sessions !== undatedSessions ||
      evidence.other_sessions !== results.sessions.filter(session => session.classification === "OtherCharger").length ||
      evidence.held_sessions !== results.sessions.length - evidence.quoted_sessions - evidence.other_sessions ||
      evidence.partial_sessions !== results.sessions.filter(session => session.pricing.state === "quoted" && session.evidence_state === "partial").length ||
      evidence.complete_zero_sessions !== results.sessions.filter(session => session.pricing.state === "quoted" && session.evidence_state === "complete_zero").length) refuse()
  const seen = new Set<string>()
  for (const [index, month] of history.months.entries()) {
    if (!hasExactKeys(month, ["month", "first_date", "last_date", "days", "periods", "total", "evidence"]) ||
        typeof month.month !== "string" || !/^\d{4}-\d{2}$/.test(month.month) || seen.has(month.month) ||
        index > 1 && month.month >= history.months[index - 1].month ||
        !date(month.first_date) || month.first_date !== `${month.month}-01` || !date(month.last_date) || month.last_date.slice(0, 7) !== month.month ||
        !validSummary(month) || !Array.isArray(month.days)) refuse()
    seen.add(month.month)
    const next = new Date(`${month.last_date}T00:00:00Z`)
    next.setUTCDate(next.getUTCDate() + 1)
    if (next.toISOString().slice(8, 10) !== "01" || month.days.length !== Number(month.last_date.slice(8))) refuse()
    if (index && !month.days.some(day => day.evidence && day.evidence.quoted_sessions + day.evidence.held_sessions + day.evidence.other_sessions > 0)) refuse()
    for (const [dayIndex, day] of month.days.entries()) {
      if (!hasExactKeys(day, ["local_date", "future", "periods", "total", "evidence"]) ||
          day.local_date !== `${month.month}-${String(dayIndex + 1).padStart(2, "0")}` ||
          typeof day.future !== "boolean" || day.future !== (day.local_date > history.today) || !validSummary(day) ||
          counts.some(key => day.evidence[key] > month.evidence[key] || month.evidence[key] > history.evidence[key])) refuse()
    }
  }
  for (const session of results.sessions) {
    for (const slice of session.pricing.quote?.slices ?? []) {
      const day = history.months.find(month => month.month === slice.local_date.slice(0, 7))?.days.find(day => day.local_date === slice.local_date)
      if (!day || !day.evidence.quoted_sessions || !day.total) refuse()
    }
  }
}

export function periodLabel(period: string) {
  return { off_peak: "Off-peak", part_peak: "Part-peak", peak: "Peak" }[period as ChargingPeriod] ?? period.replaceAll("_", " ")
}

export function dayLabel(day: ChargingDay) {
  const evidence = day.evidence
  return `${day.local_date}: ${day.total ? `${day.total.ac_kwh} kWh, estimated charging cost ${day.total.display_cost}` : "No quoted charging"}${day.future ? " · Future day" : ""}${evidence.complete_zero_sessions ? " · Complete zero-energy evidence" : ""}${evidence.partial_sessions ? " · Partial coverage" : ""}${evidence.held_sessions ? " · Held sessions excluded" : ""}${evidence.other_sessions ? " · Other charger excluded" : ""}`
}
