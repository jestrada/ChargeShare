import { useEffect, useState } from "react"
import { Button } from "./components/ui/button"
import { classificationLabel, evidenceLabel, parseResults, reasonLabel, resultPath, scopes, type Scope, type ScopedResults } from "./persisted"

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
    <Provenance results={results} />
  </section>
  return <section className="glass persisted-panel">
    <h2>Committed session evidence</h2>
    <dl className="persisted-energy">
      <div><dt>Observed vehicle-reported AC</dt><dd>{results.observed_ac_kwh} <span>kWh</span></dd></div>
      <div><dt>Eligible shared-charger energy</dt><dd>{results.eligible_shared_charger_kwh} <span>kWh eligible</span></dd></div>
    </dl>
    <p className="muted">Charger classification updates automatically from session location evidence.</p>
    {results.sessions.map(session => <details className="persisted-session" key={`${session.vehicle}/${session.connection}`}>
      <summary><span>{session.connection}</span><span>{session.observed_ac_kwh} kWh observed AC</span><span>{classificationLabel(session)}</span><span>{evidenceLabel(session)}</span><span>Quality and provenance details</span></summary>
      <div className="persisted-detail">
        <p>Session identity: {session.vehicle} / {session.connection}</p>
        <p>{evidenceLabel(session)}</p>
        <p>{reasonLabel(session.classification_reason)}</p>
        <p>{session.eligible_shared_charger_kwh} kWh eligible</p>
        {session.exclusion_reasons.map(reason => <p className="needs-review" key={reason}>{reasonLabel(reason)}</p>)}
        {session.quality_flags.length ? <ul>{session.quality_flags.map(flag => <li key={flag}>{reasonLabel(flag)}</li>)}</ul> : <p className="muted">No counter or boundary quality flags.</p>}
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
