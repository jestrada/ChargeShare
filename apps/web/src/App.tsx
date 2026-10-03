import { useEffect, useState, type FormEvent } from "react"
import { ChevronDownIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { HelpTooltip } from "@/components/help-tooltip"
import { PreviewSettings } from "@/components/preview-settings"
import { isVehicleView, readPreferences, savePreferences, vehicleOptions, type Preferences } from "./preferences"
import type { DemoSnapshot, Scenario } from "./demo"

const scenarioOptions = [
  { value: "complete", label: "Complete readings" },
  { value: "missing", label: "Missing reading" },
] as const

export function App() {
  const [scenario, setScenario] = useState<Scenario>("complete")
  const [selectedScenario, setSelectedScenario] = useState<Scenario>("complete")
  const [reload, setReload] = useState(0)
  const [preferences, setPreferences] = useState(readPreferences)
  const [storageAvailable, setStorageAvailable] = useState(true)
  const [vehicleId, setVehicleId] = useState(preferences.defaultVehicle)
  const [snapshot, setSnapshot] = useState<DemoSnapshot | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    const controller = new AbortController()
    setSnapshot(null)
    setError(false)
    async function load() {
      try {
        const response = await fetch(`/api/demo?scenario=${scenario}`, { signal: controller.signal })
        if (!response.ok) throw new Error("Demo unavailable")
        const data: DemoSnapshot = await response.json()
        if (!data.periods || !data.rates?.current || !data.rates.next || !data.totals?.week || !data.totals.month) {
          throw new Error("Preview API needs to be restarted")
        }
        if (!controller.signal.aborted) setSnapshot(data)
      } catch {
        if (!controller.signal.aborted) setError(true)
      }
    }
    void load()
    return () => controller.abort()
  }, [scenario, reload])

  function loadScenario(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setScenario(selectedScenario)
    setReload((value) => value + 1)
  }

  function updatePreferences(change: Partial<Preferences>) {
    const next = { ...preferences, ...change }
    setPreferences(next)
    setStorageAvailable(savePreferences(next))
    if (change.defaultVehicle !== undefined) setVehicleId(change.defaultVehicle)
  }

  const vehicles = snapshot?.vehicles.filter((vehicle) => vehicleId === "all" || vehicle.id === vehicleId) ?? []
  const totals = vehicleId === "all" ? snapshot?.totals : vehicles[0]
  const sessions = vehicles.flatMap((vehicle) => vehicle.sessions)
    .sort((left, right) => right.date_order - left.date_order || left.vehicle.localeCompare(right.vehicle))

  return (
    <div className="app-shell" data-charging-motion={preferences.animateCharging ? "on" : "off"}>
      <a className="skip-link" href="#main">Skip to charging</a>
      <main id="main" className="main-content">
        <div className="page-heading">
          <h1>ChargeShare</h1>
          <div className="page-actions">
            <p className="muted">Demo data · Vehicles not connected</p>
            <PreviewSettings preferences={preferences} onChange={updatePreferences} storageAvailable={storageAvailable} />
          </div>
        </div>
        {snapshot && totals ? (
          <>
            <section className="vehicle-grid" aria-label="Demo vehicle batteries">
              {snapshot.vehicles.map((vehicle) => (
                <article key={vehicle.id} className={`glass vehicle-panel ${vehicle.state === "Charging" ? "is-charging" : ""}`}>
                  {vehicle.state === "Charging" && <div className="charging-rays" aria-hidden="true"><span /><span /><span /></div>}
                  <div className="vehicle-heading"><h2>{vehicle.model}</h2><p className="muted">{vehicle.name}</p></div>
                  <div className="battery-reading">
                    <p className="battery-number">{vehicle.battery}<span>%</span></p>
                    <p className="charging-state">{vehicle.state}</p>
                  </div>
                  <div className="battery-glass">
                    <progress max="100" value={vehicle.battery} aria-label={`${vehicle.name} demo battery`} />
                    {vehicle.state === "Charging" && (
                      <div className="charge-track" aria-hidden="true">
                        <div className="charge-fill" style={{ width: `${vehicle.battery}%` }}><span className="charge-reflection" /></div>
                      </div>
                    )}
                  </div>
                  <div className="battery-scale" aria-hidden="true"><span>0%</span><span>100%</span></div>
                </article>
              ))}
            </section>
            <section className="glass rate-panel" aria-labelledby="rate-heading">
              <div className="rate-heading">
                <div className="heading-with-help"><h2 id="rate-heading">Electricity rate</h2><HelpTooltip label="About sample rates">Fictional prices follow the sample clock. Your utility tariff is not connected.</HelpTooltip></div>
                <p className="muted">Sample clock · {snapshot.periods.as_of}</p>
              </div>
              <dl className="rate-grid">
                <div><dt>Current price</dt><dd>{snapshot.rates.current.price}<span> / kWh</span></dd><p className="muted">Until {snapshot.rates.current.until}</p></div>
                <div><dt>Next window</dt><dd>{snapshot.rates.next.price}<span> / kWh</span></dd><p className="muted">{snapshot.rates.next.starts} to {snapshot.rates.next.ends}</p></div>
              </dl>
            </section>
            <section className="glass ledger-panel" aria-labelledby="cost-heading">
              <div className="section-heading">
                <div className="heading-with-help"><h2 id="cost-heading">Charging costs</h2><HelpTooltip label="How charging totals work">Only priced sessions within these dates are included. Open a session to see its rate breakdown.</HelpTooltip></div>
              </div>
              <Tabs value={vehicleId} onValueChange={(value) => { if (isVehicleView(value)) setVehicleId(value) }} className="vehicle-tabs">
                <TabsList aria-label="Charging costs by vehicle" activateOnFocus>
                  {vehicleOptions.map((option) => <TabsTrigger key={option.value} value={option.value}>{option.label}</TabsTrigger>)}
                </TabsList>
                {vehicleOptions.map((option) => (
                  <TabsContent key={option.value} value={option.value}>
                    {vehicleId === option.value && <>
                      <dl className="cost-summary" aria-live="polite">
                        <div><dt>This week</dt><dd>{totals.week.priced_subtotal}{" "}<span>{totals.week.priced_energy} kWh</span></dd><p className="muted">{snapshot.periods.week}</p></div>
                        <div><dt>This month</dt><dd>{totals.month.priced_subtotal}{" "}<span>{totals.month.priced_energy} kWh</span></dd><p className="muted">{snapshot.periods.month} · Month to date</p></div>
                      </dl>
                      {totals.month.held > 0 && <p className="review-note" role="status">{totals.month.held} {totals.month.held === 1 ? "session needs" : "sessions need"} review. Only priced sessions are included in these totals.</p>}
                      <div className="session-heading"><h3>Sessions</h3><p className="muted">September · Sample period</p></div>
                      <div className="session-columns muted" aria-hidden="true"><span>Session</span><span>Duration</span><span>Energy</span><span>Cost</span></div>
                      <div className="session-list">
                        {sessions.map((session) => (
                          <details key={`${snapshot.scenario}-${session.id}`} className="session">
                            <summary>
                              <span className="session-date">{session.date}<span className="muted">{session.vehicle}</span></span>
                              <span className="session-time muted">{session.time}</span>
                              <span className="session-energy">{session.energy} kWh</span>
                              <span className={`session-cost ${session.cost === null ? "needs-review" : ""}`}>{session.cost ?? "Needs review"}</span>
                              <ChevronDownIcon className="session-caret" aria-hidden="true" />
                            </summary>
                            <div className="session-detail">
                              <p className="muted">Shared charger · AC energy</p>
                              {session.reason ? <p>{session.reason}</p> : (
                                <Table>
                                  <TableHeader><TableRow><TableHead>Energy</TableHead><TableHead>Sample rate</TableHead><TableHead className="text-right">Cost</TableHead></TableRow></TableHeader>
                                  <TableBody>{session.lines.map((line, index) => (
                                    <TableRow key={`${line.version}-${index}`}>
                                      <TableCell>{line.energy} kWh</TableCell><TableCell>{line.rate}</TableCell><TableCell className="text-right">{line.cost}</TableCell>
                                    </TableRow>
                                  ))}</TableBody>
                                </Table>
                              )}
                            </div>
                          </details>
                        ))}
                      </div>
                      <p className="ledger-note muted">Sample rates, not your utility tariff. Costs include priced sessions only.</p>
                    </>}
                  </TabsContent>
                ))}
              </Tabs>
            </section>
          </>
        ) : (
          <section className="glass loading-panel" aria-live="polite">
            {error ? <><h2>Sample data is unavailable</h2><p>Check that the local preview server is running, then load the scenario again.</p></> : <p>Loading sample data…</p>}
          </section>
        )}
        <section className="sample-controls" aria-labelledby="sample-heading">
          <div><h2 id="sample-heading">Sample data</h2><p className="muted" aria-live="polite">{snapshot ? snapshot.scenario === "complete" ? "Showing complete readings." : "Showing one missing rate-change reading for Evan." : "Local preview"}</p></div>
          <form onSubmit={loadScenario}>
            <label className="sr-only" htmlFor="scenario">Sample scenario</label>
            <Select items={scenarioOptions} value={selectedScenario} onValueChange={(value) => { if (value === "complete" || value === "missing") setSelectedScenario(value) }}>
              <SelectTrigger id="scenario"><SelectValue /></SelectTrigger>
              <SelectContent align="end" alignItemWithTrigger={false} sideOffset={8}>
                <SelectGroup>{scenarioOptions.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectGroup>
              </SelectContent>
            </Select>
            <Button className="control" type="submit" disabled={!snapshot && !error}>Load scenario</Button>
          </form>
        </section>
      </main>
    </div>
  )
}
