import { useEffect, useState } from "react"
import { Button } from "./components/ui/button"
import { evidenceLabel, observationTime, pricingHoldLabel, parseResults, parseScopes, reasonLabel, resultPath, type Scope, type CarScope, type ScopedResults, periodLabel } from "./persisted"

import { CarCards } from "./components/car-cards"

import { MonthlyCharging } from "./components/monthly-charging"

type View = { scope: string; state: "loading" | "unavailable" | "refused" } |
  { scope: string; state: "available"; results: ScopedResults }

function initialScope(): Scope | null {
  const params = new URLSearchParams(window.location.search)
  if (!params.has("owner") && !params.has("vehicle")) return null
  return { owner: params.get("owner") ?? "", vehicle: params.get("vehicle") ?? "" }
}

export function PersistedApp() {
  const [scope, setScope] = useState(initialScope)
  const [charger, setCharger] = useState<string>("")
  const [timezone, setTimezone] = useState<string | null>(null)
  const [refresh, setRefresh] = useState(0)
  const [configured, setConfigured] = useState<{ synthetic_only: boolean; scopes: CarScope[] } | null>(null)
  const scopeKey = scope ? `${scope.owner}/${scope.vehicle}` : ""
  const key = `${scopeKey}/${charger}`
  const scopesAvailable = configured !== null
  const [view, setView] = useState<View>({ scope: key, state: "loading" })
  useEffect(() => {
    const controller = new AbortController()
    let active = true
    const timeout = window.setTimeout(() => controller.abort(), 5000)
    fetch("/api/poc/scopes", { signal: controller.signal, cache: "no-store" })
      .then(async response => {
        if (!response.ok) throw new Error("Persisted scopes unavailable")
        const next = parseScopes(await response.json())
        if (active) {
          setConfigured(next)
          setScope(previous => previous && next.scopes.some(option => option.owner === previous.owner && option.vehicle === previous.vehicle) ? next.scopes.find(option => option.vehicle === previous.vehicle)! : next.scopes[0] ?? null)
        }
      }).catch(() => {
        if (active) { setConfigured(null); setView({ scope: key, state: "unavailable" }) }
      }).finally(() => window.clearTimeout(timeout))
    return () => { active = false; controller.abort(); window.clearTimeout(timeout) }
  }, [refresh])
  useEffect(() => {
    const poll = window.setTimeout(() => setRefresh(value => value + 1), 10000)
    if (!scope || !scopesAvailable) return () => window.clearTimeout(poll)
    const controller = new AbortController()
    let active = true
    setView(previous => previous.scope === key && previous.state === "available" ? previous : { scope: key, state: "loading" })
    const timeout = window.setTimeout(() => controller.abort(), 5000)
    fetch(resultPath(scope, charger), { signal: controller.signal, cache: "no-store" })
      .then(async response => {
        if (!response.ok) {
          if (active) setView({ scope: key, state: response.status === 400 || response.status === 404 ? "refused" : "unavailable" })
          return
        }
        const results = parseResults(await response.json(), scope)
        if (active) {
          setTimezone(results.monthly.timezone)
          setView({ scope: key, state: "available", results })
        }
      })
      .catch(() => { if (active) setView({ scope: key, state: "unavailable" }) })
      .finally(() => window.clearTimeout(timeout))
    return () => { active = false; controller.abort(); window.clearTimeout(timeout); window.clearTimeout(poll) }
  }, [scope, key, charger, refresh, scopesAvailable])
  const current: View = !scopesAvailable && view.state === "available" ? { scope: key, state: "unavailable" } :
    view.scope === key ? view : { scope: key, state: "loading" }
  function chooseScope(next: Scope) {
    setScope(next)
    setCharger("")
    const params = new URLSearchParams({ owner: next.owner, vehicle: next.vehicle })
    window.history.replaceState(null, "", `/?${params}`)
  }
  return <div className="app-shell persisted-shell">
    <a className="skip-link" href="#persisted-results">Skip to persisted results</a>
    <header className="persisted-heading">
      <div><p className="persisted-label">{configured?.synthetic_only ? "Synthetic evidence" : "Charging history"}</p><h1>ChargeShare</h1></div>
      <div className="persisted-controls">
        <label className="persisted-selector">Charger
          <select aria-label="Charger" value={charger} disabled={!scope || !configured?.scopes.length} onChange={event => setCharger(event.target.value)}>
            <option value="">All chargers</option>
            {(scope?.chargers ?? []).map(option => <option key={option.id} value={option.id}>{option.name}</option>)}
          </select>
        </label>
        <Button className="control" variant="outline" onClick={() => setRefresh(value => value + 1)}>Refresh</Button>
      </div>
    </header>
    {configured && <CarCards cars={configured.scopes} selected={scopeKey} timezone={timezone} chooseScope={chooseScope} />}
    <main id="persisted-results" aria-live="polite" aria-busy={configured?.scopes.length !== 0 && current.state === "loading"}>
      {configured?.scopes.length === 0 ? <section className="glass persisted-panel"><h2>No active cars</h2><p>Add a car with the car command while ingestion is stopped, then restart to recover its available history.</p></section> : <>
      {current.state === "loading" && <section className="glass persisted-panel"><h2>Reading committed evidence…</h2></section>}
      {current.state === "unavailable" && <section className="glass persisted-panel" role="alert"><h2>Persisted API unavailable</h2><p>Committed results could not be read. Refresh when the owning runtime is ready.</p></section>}
      {current.state === "refused" && <section className="glass persisted-panel" role="alert"><h2>Scope refused</h2><p>Select a configured owner and vehicle pair to read its evidence.</p></section>}
      {current.state === "available" && <Results key={key} results={current.results} carName={scope?.name ?? "Charging history"} chargerName={scope?.chargers?.find(option => option.id === charger)?.name ?? "All chargers"} chargers={scope?.chargers ?? []} />}
      </>}
    </main>
  </div>
}

function Results({ results, carName, chargerName, chargers }: { results: ScopedResults; carName: string; chargerName: string; chargers: { id: string; name: string }[] }) {
  return <section className="glass persisted-panel">
    <h2>{results.registry?.car_name ?? carName} · {chargerName}</h2>
    <MonthlyCharging history={results.monthly} />
    {results.state === "empty" && <div><h2>No committed sessions</h2><p>This vehicle has no retained session evidence yet.</p></div>}
    <p className="muted">Vehicle-reported AC input counter energy; physical accuracy unvalidated. Quoted energy is allocated by elapsed UTC time.</p>
    <p className="muted">{results.pricing.estimate_boundary}</p>
    <Configuration results={results} chargers={chargers} />
    {!!results.sessions.length && <details className="persisted-history">
      <summary>Session information ({results.sessions.length})</summary>
      {results.sessions.map((session, index) => <details className="persisted-session" key={`${session.vehicle}/${session.connection}`}>
        <summary><span>Session {index + 1} · {observationTime(session.start_utc_seconds)}</span><span>{session.observed_ac_kwh} kWh observed AC</span><span>{session.classification === "SharedCharger" ? (session.charger?.name ?? chargers.find(charger => charger.id === results.pricing.charger_id)?.name ?? "Shared charger") : session.classification === "OtherCharger" ? "Other charger · excluded" : "Held"}</span><span>{evidenceLabel(session)}</span><span>{session.pricing.quote ? `Estimated charging cost ${session.pricing.quote.display_cost}` : pricingHoldLabel(session.pricing.hold_reason)}</span><span>View session details</span></summary>
        <div className="persisted-detail">
          <p>{evidenceLabel(session)}</p>
          <p>Connection: {observationTime(session.start_utc_seconds)} → {observationTime(session.end_utc_seconds)}.</p>
          {session.coverage && <div className="persisted-coverage">
            <p>Observed counter window: {observationTime(session.coverage.first_counter_utc_seconds)} → {observationTime(session.coverage.last_counter_utc_seconds)}; {session.coverage.usable_readings} usable readings.</p>
            {(session.coverage.partial_start || session.coverage.partial_end) && <p className="needs-review">Partial coverage: {session.coverage.partial_start ? "energy before the first counter is unknown. " : ""}{session.coverage.partial_end ? "Energy after the last counter is unknown." : ""} No energy is inferred at connection boundaries.</p>}
            <p className="muted">{session.coverage.quantization}</p>
          </div>}
          <p>{reasonLabel(session.classification_reason)}</p>
          <p>{session.eligible_shared_charger_kwh} kWh eligible</p>
          {session.exclusion_reasons.map(reason => <p className="needs-review" key={reason}>{reasonLabel(reason)}</p>)}
          {session.quality_flags.length ? <ul>{session.quality_flags.map(flag => <li key={flag}>{reasonLabel(flag)}</li>)}</ul> : <p className="muted">No counter or boundary quality flags.</p>}
          {session.pricing.quote ? <div className="persisted-quote">
            <p>Estimated charging cost: {session.pricing.quote.display_cost}</p>
            {session.pricing.quote.slices.map((slice, sliceIndex) => <p key={sliceIndex}>
              {slice.local_start} → {slice.local_end} · {periodLabel(slice.period)} · {slice.allocated_ac_kwh} kWh · {slice.usd_per_kwh}
            </p>)}
          </div> : <p className="needs-review">{pricingHoldLabel(session.pricing.hold_reason)}</p>}
          <p>{results.evidence_boundary}</p>
        </div>
      </details>)}
    </details>}
  </section>
}

function Configuration({ results, chargers }: { results: ScopedResults; chargers: { id: string; name: string }[] }) {
  return <section className="persisted-rates" aria-label="Configuration">
    <h2>Configuration</h2>
    <p>Charger: {results.registry ? results.registry.chargers.filter(charger => results.registry?.selected_charger === null || charger.id === results.registry?.selected_charger).map(charger => charger.name).join(", ") || "No charger selected" : chargers.map(charger => charger.name).join(", ") || "No charger selected"}</p>
    {results.pricing.rate_models.map(model => <details key={model.id}>
      <summary>{model.plan} · {model.timezone}</summary>
      <dl className="persisted-provenance">
        <div><dt>Public rate source</dt><dd><a href={model.source_url}>{model.source_label}</a></dd></div>
        <div><dt>Effective local dates [from, until)</dt><dd>{model.effective_from} → {model.effective_until ?? "Open-ended"}</dd></div>
        <div><dt>Estimated energy rates · USD/kWh</dt><dd><ul>{model.periods.map((period, index) => <li key={index}>{period.season} / {periodLabel(period.period)} / {period.usd_per_kwh}</li>)}</ul></dd></div>
      </dl>
    </details>)}
  </section>
}
