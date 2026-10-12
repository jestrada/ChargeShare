export type Scope = { owner: string; vehicle: string }
export type SessionResult = {
  vehicle: string
  connection: string
  classification: "Unconfirmed"
  evidence_state: "complete" | "complete_zero" | "incomplete"
  observed_ac_kwh: string
  eligible_shared_charger_kwh: string
  quality_flags: string[]
  exclusion_reasons: string[]
}
export type ScopedResults = Scope & {
  state: "empty" | "held"
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
  }
}
export const scopes: Scope[] = [
  { owner: "owner-a", vehicle: "vehicle-a" },
  { owner: "owner-b", vehicle: "vehicle-b" },
]
export function resultPath(scope: Scope) {
  return `/api/poc/owners/${encodeURIComponent(scope.owner)}/vehicles/${encodeURIComponent(scope.vehicle)}/results`
}
export function parseResults(value: unknown, scope: Scope): ScopedResults {
  const object = value as ScopedResults
  const energy = (value: unknown) => typeof value === "string" && /^\d+\.\d{6}$/.test(value)
  const reasons = (value: unknown) => Array.isArray(value) && value.every(reason => typeof reason === "string" && /^[A-Za-z()]+$/.test(reason))
  if (!object || object.owner !== scope.owner || object.vehicle !== scope.vehicle ||
      !["empty", "held"].includes(object.state) || !energy(object.observed_ac_kwh) ||
      object.eligible_shared_charger_kwh !== "0.000000" ||
      typeof object.evidence_boundary !== "string" || !object.evidence_boundary.includes("physical accuracy unvalidated") ||
      typeof object.revision !== "string" || !/^results-v1-[a-f0-9]{16}$/.test(object.revision) ||
      !Array.isArray(object.sessions) || (object.state === "empty") !== (object.sessions.length === 0) ||
      !object.provenance || object.provenance.source !== "synthetic-receiver-kafka" ||
      !["source", "source_epoch", "input_contract", "mapping_version", "manifest_version", "normalizer_version", "calculation_version"].every(key =>
        typeof object.provenance[key as keyof ScopedResults["provenance"]] === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(object.provenance[key as keyof ScopedResults["provenance"]])) ||
      object.sessions.some(session => !session || session.vehicle !== scope.vehicle ||
        !/^[A-Za-z0-9_-]{1,64}$/.test(session.connection) || session.classification !== "Unconfirmed" ||
        !["complete", "complete_zero", "incomplete"].includes(session.evidence_state) ||
        !energy(session.observed_ac_kwh) || session.eligible_shared_charger_kwh !== "0.000000" ||
        !reasons(session.quality_flags) || !reasons(session.exclusion_reasons) ||
        !session.exclusion_reasons.includes("UnconfirmedCharger"))) {
    throw new Error("Persisted results unavailable")
  }
  return object
}
export function evidenceLabel(session: SessionResult) {
  if (session.evidence_state === "incomplete") return "Incomplete evidence"
  if (session.evidence_state === "complete_zero") return "Complete zero-energy evidence"
  return "Complete counter evidence"
}
export function reasonLabel(reason: string) {
  if (reason === "UnconfirmedCharger") return "Held: charger has not been confirmed as the shared charger."
  return reason.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/[()]/g, " ").trim()
}
