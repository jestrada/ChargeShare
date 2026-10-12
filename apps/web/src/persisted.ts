export type Scope = { owner: string; vehicle: string }
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
}
export type ScopedResults = Scope & {
  state: "empty" | "held" | "eligible"
  evidence_boundary: string
  observed_ac_kwh: string
  eligible_shared_charger_kwh: string
  sessions: SessionResult[]
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
const resultKeys = ["owner", "vehicle", "state", "evidence_boundary", "observed_ac_kwh", "eligible_shared_charger_kwh", "sessions", "revision", "provenance"]
const sessionKeys = ["vehicle", "connection", "classification", "classification_reason", "evidence_state", "observed_ac_kwh", "eligible_shared_charger_kwh", "quality_flags", "exclusion_reasons"]
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
export function parseResults(value: unknown, scope: Scope): ScopedResults {
  const object = value as ScopedResults
  const energy = (value: unknown) => typeof value === "string" && /^\d+\.\d{6}$/.test(value)
  const reasons = (value: unknown, allowed: string[]) => Array.isArray(value) && value.every(reason => allowed.includes(reason))
  if (!hasExactKeys(object, resultKeys) || object.owner !== scope.owner || object.vehicle !== scope.vehicle ||
      !["empty", "held", "eligible"].includes(object.state) || !energy(object.observed_ac_kwh) || !energy(object.eligible_shared_charger_kwh) ||
      object.evidence_boundary !== "Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated" ||
      typeof object.revision !== "string" || !/^results-v1-[a-f0-9]{16}$/.test(object.revision) ||
      !Array.isArray(object.sessions) || (object.state === "empty") !== (object.sessions.length === 0) ||
      !hasExactKeys(object.provenance, provenanceKeys) || object.provenance.source !== "synthetic-receiver-kafka" ||
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
