import { useEffect, useState } from "react"
import { Button } from "./components/ui/button"
import { classificationLabel, evidenceLabel, pricingHoldLabel, parseResults, reasonLabel, resultPath, scopes, type Scope, type ScopedResults } from "./persisted"

type View = { scope: string; state: "loading" | "unavailable" | "refused" } |
  { scope: string; state: "available"; results: ScopedResults }

function initialScope(): Scope {
  const params = new URLSearchParams(window.location.search)
  return { owner: params.get("owner") ?? scopes[0].owner, vehicle: params.get("vehicle") ?? scopes[0].vehicle }
}

export function PersistedApp() {
  const [scope, setScope] = useState(initialScope)
  const [refresh, setRefresh] = useState(0)
  const key = `${scope.owner}/${scope.vehicle}`
  const [view, setView] = useState<View>({ scope: key, state: "loading" })
  useEffect(() => {
    const controller = new AbortController()
    let active = true
    setView(previous => previous.scope === key && previous.state === "available" ? previous : { scope: key, state: "loading" })
    const timeout = window.setTimeout(() => controller.abort(), 5000)
    fetch(resultPath(scope), { signal: controller.signal, cache: "no-store" })
      .then(async response => {
        if (!response.ok) {
          if (active) setView({ scope: key, state: response.status === 400 || response.status === 404 ? "refused" : "unavailable" })
          return
        }
        const results = parseResults(await response.json(), scope)
        if (active) setView({ scope: key, state: "available", results })
      })
      .catch(() => { if (active) setView({ scope: key, state: "unavailable" }) })
      .finally(() => window.clearTimeout(timeout))
    const poll = window.setTimeout(() => setRefresh(value => value + 1), 10000)
    return () => { active = false; controller.abort(); window.clearTimeout(timeout); window.clearTimeout(poll) }
  }, [scope, key, refresh])
  const current: View = view.scope === key ? view : { scope: key, state: "loading" }
  function chooseScope(next: Scope) {
    setScope(next)
    const params = new URLSearchParams({ owner: next.owner, vehicle: next.vehicle })
    window.history.replaceState(null, "", `/?${params}`)
  }
  return <div className="app-shell persisted-shell">
    <a className="skip-link" href="#persisted-results">Skip to persisted results</a>
    <header className="persisted-heading">
      <div><p className="persisted-label">Persisted · Synthetic</p><h1>Charging evidence</h1></div>
      <Button className="control" variant="outline" onClick={() => setRefresh(value => value + 1)}>Refresh results</Button>
    </header>
    <section className="glass persisted-panel" aria-label="Owner and vehicle scope">
      <h2>View one owner and vehicle</h2>
      <div className="persisted-scopes" role="group" aria-label="Select scope">
        {scopes.map(option => <Button key={option.vehicle} className="control" variant={scope.owner === option.owner && scope.vehicle === option.vehicle ? "default" : "outline"}
          aria-pressed={scope.owner === option.owner && scope.vehicle === option.vehicle} onClick={() => chooseScope(option)}>
          {option.owner} / {option.vehicle}
        </Button>)}
      </div>
      <p className="persisted-scope">Selected scope: <strong>{scope.owner} / {scope.vehicle}</strong></p>
      <p className="muted">Synthetic vehicle-reported AC energy; physical accuracy unvalidated.</p>
    </section>
    <main id="persisted-results" aria-live="polite" aria-busy={current.state === "loading"}>
      {current.state === "loading" && <section className="glass persisted-panel"><h2>Reading committed evidence…</h2></section>}
      {current.state === "unavailable" && <section className="glass persisted-panel" role="alert"><h2>Persisted API unavailable</h2><p>Committed results could not be read. Refresh when the owning runtime is ready.</p></section>}
      {current.state === "refused" && <section className="glass persisted-panel" role="alert"><h2>Scope refused</h2><p>Select a configured owner and vehicle pair to read its evidence.</p></section>}
      {current.state === "available" && <Results results={current.results} />}
    </main>
  </div>
}

function Results({ results }: { results: ScopedResults }) {
  if (results.state === "empty") return <section className="glass persisted-panel">
    <h2>No committed sessions</h2><p>This scope has no retained session evidence yet. Start the finite receiver check to populate synthetic evidence.</p>
    <RateProvenance results={results} />
    <Provenance results={results} />
  </section>
  return <section className="glass persisted-panel">
    <h2>Committed session evidence</h2>
    <dl className="persisted-energy">
      <div><dt>Observed vehicle-reported AC</dt><dd>{results.observed_ac_kwh} <span>kWh</span></dd></div>
      <div><dt>Eligible shared-charger energy</dt><dd>{results.eligible_shared_charger_kwh} <span>kWh eligible</span></dd></div>
    </dl>
    <dl className="persisted-energy">
      <div><dt>Quoted energy subtotal · rounded once</dt><dd>{results.pricing.display_subtotal ?? "No quoted subtotal"}</dd></div>
      <div><dt>Quoted vehicle-reported AC</dt><dd>{results.pricing.priced_ac_kwh} <span>kWh</span></dd></div>
    </dl>
    {results.pricing.exact_subtotal && <p>Exact scoped subtotal: {results.pricing.exact_subtotal}</p>}
    <p>{results.pricing.quoted_session_count} quoted sessions · {results.pricing.rate_held_session_count} rate-held · {results.pricing.ineligible_session_count} ineligible</p>
    <p className="muted">{results.pricing.estimate_boundary}</p>
    <RateProvenance results={results} />
    <p className="muted">Charger classification updates automatically from session location evidence.</p>
    {results.sessions.map(session => <details className="persisted-session" key={`${session.vehicle}/${session.connection}`}>
      <summary><span>{session.connection}</span><span>{session.observed_ac_kwh} kWh observed AC</span><span>{classificationLabel(session)}</span><span>{evidenceLabel(session)}</span><span>{session.pricing.quote?.display_cost ?? pricingHoldLabel(session.pricing.hold_reason)}</span><span>Quality, price and provenance details</span></summary>
      <div className="persisted-detail">
        <p>Session identity: {session.vehicle} / {session.connection}</p>
        <p>{evidenceLabel(session)}</p>
        <p>{reasonLabel(session.classification_reason)}</p>
        <p>{session.eligible_shared_charger_kwh} kWh eligible</p>
        {session.exclusion_reasons.map(reason => <p className="needs-review" key={reason}>{reasonLabel(reason)}</p>)}
        {session.quality_flags.length ? <ul>{session.quality_flags.map(flag => <li key={flag}>{reasonLabel(flag)}</li>)}</ul> : <p className="muted">No counter or boundary quality flags.</p>}
        {session.pricing.quote ? <div className="persisted-quote">
          <p>Session price: {session.pricing.quote.display_cost} · exact {session.pricing.quote.exact_cost}</p>
          {session.pricing.quote.slices.map((slice, index) => <dl className="persisted-provenance" key={index}>
            <div><dt>UTC interval · epoch seconds [start, end)</dt><dd>{slice.start_utc_seconds} → {slice.end_utc_seconds}</dd></div>
            <div><dt>Local interval [start, end)</dt><dd>{slice.local_start} → {slice.local_end}</dd></div>
            <div><dt>Local date / UTC offset seconds</dt><dd>{slice.local_date} / {slice.utc_offset_seconds}</dd></div>
            <div><dt>Season / day / period</dt><dd>{slice.season} / {slice.day_type} / {slice.period}</dd></div>
            <div><dt>Model version</dt><dd>{slice.model_version}</dd></div>
            <div><dt>Allocated AC energy</dt><dd>{slice.allocated_ac_kwh} kWh</dd></div>
            <div><dt>Rate / exact cost</dt><dd>{slice.usd_per_kwh} / {slice.exact_cost}</dd></div>
          </dl>)}
        </div> : <p className="needs-review">{pricingHoldLabel(session.pricing.hold_reason)}</p>}
        <p>{results.evidence_boundary}</p>
        <Provenance results={results} />
      </div>
    </details>)}
    <p className="muted persisted-revision">Semantic revision: {results.revision}</p>
  </section>
}

function Provenance({ results }: { results: ScopedResults }) {
  return <dl className="persisted-provenance">
    <div><dt>Source</dt><dd>{results.provenance.source}</dd></div>
    <div><dt>Source epoch</dt><dd>{results.provenance.source_epoch}</dd></div>
    <div><dt>Input contract</dt><dd>{results.provenance.input_contract}</dd></div>
    <div><dt>Mapping / manifest</dt><dd>{results.provenance.mapping_version} / {results.provenance.manifest_version}</dd></div>
    <div><dt>Normalization / calculation</dt><dd>{results.provenance.normalizer_version} / {results.provenance.calculation_version}</dd></div>
    <div><dt>Geofence configuration</dt><dd>{results.provenance.geofence_version}</dd></div>
    <div><dt>Revision</dt><dd>{results.revision}</dd></div>
  </dl>
}

function RateProvenance({ results }: { results: ScopedResults }) {
  return <div className="persisted-rates">
    <p>Charger: {results.pricing.charger_id ?? "No rate configured"} · calculation: {results.pricing.calculation_version}</p>
    {results.pricing.rate_models.map(model => <details key={model.id}>
      <summary>{model.plan} · {model.id} · {model.timezone}</summary>
      <dl className="persisted-provenance">
        <div><dt>Public sample source</dt><dd><a href={model.source_url}>{model.source_label}</a></dd></div>
        <div><dt>Effective local dates [from, until)</dt><dd>{model.effective_from} → {model.effective_until ?? "Open-ended"}</dd></div>
        <div><dt>Timezone / season / day rules</dt><dd>{model.timezone} / {model.timezone_rules_version} / {model.season_rules_version} / {model.day_type_rules_version}</dd></div>
        <div><dt>Named sample rates · USD/kWh</dt><dd><ul>{model.periods.map((period, index) => <li key={index}>{period.season} / {period.period} / {period.usd_per_kwh}</li>)}</ul></dd></div>
        <div><dt>Holiday labels · all days share rates</dt><dd>{model.holidays.join(", ") || "None configured"}</dd></div>
      </dl>
    </details>)}
  </div>
}
