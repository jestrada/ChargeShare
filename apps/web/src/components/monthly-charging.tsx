import { useState, type KeyboardEvent } from "react"
import { scaleBand, scaleLinear } from "d3-scale"
import { stack } from "d3-shape"
import { Button } from "./ui/button"
import { chargingPeriods, dayLabel, periodLabel, reasonLabel, type ChargingDay, type ChargingMonth, type MonthlyHistory } from "../persisted"

type BarUnit = "cost" | "kWh"

function monthLabel(month: string) {
  return new Date(`${month}-01T12:00:00Z`).toLocaleDateString("en-US", { month: "long", year: "numeric", timeZone: "UTC" })
}

function DayDetails({ day }: { day: ChargingDay }) {
  return <div className="charging-day-detail" aria-live="polite">
    <h3>{day.local_date}</h3>
    <p>{dayLabel(day)}</p>
    {day.total && <dl className="charging-breakdown">
      {day.periods.map(period => <div key={period.period}><dt>{periodLabel(period.period)}</dt><dd>{period.ac_kwh} kWh · {period.display_cost}</dd></div>)}
      <div><dt>Day total</dt><dd>{day.total.ac_kwh} kWh · {day.total.display_cost}</dd></div>
    </dl>}
    {day.evidence.partial_sessions > 0 && <p className="needs-review">Partial coverage: only the observed counter window is included; energy outside it is unknown.</p>}
    {day.evidence.held_sessions > 0 && <p className="muted">{day.evidence.held_sessions} held sessions excluded from the chart. Reasons are available in session information.</p>}
    {day.evidence.quality_flags.length > 0 && <p className="muted">{day.evidence.quality_flags.map(reasonLabel).join(" · ")}</p>}
  </div>
}

function MonthChart({ month, unit, selected, select }: { month: ChargingMonth; unit: BarUnit; selected: string; select: (date: string) => void }) {
  const x = scaleBand<string>().domain(month.days.map(day => day.local_date)).range([40, 740]).padding(0.2)
  const layers = stack<ChargingDay, string>().keys([...chargingPeriods]).value((day, period) => {
    const total = day.periods.find(total => total.period === period)!
    return unit === "cost" ? Number(total.exact_cost.replace(" USD", "")) : Number(total.ac_kwh)
  })(month.days)
  const maximum = Math.max(0, ...layers.flatMap(layer => layer.map(point => point[1])))
  const y = scaleLinear().domain([0, maximum || 1]).range([190, 15]).nice()
  const day = month.days.find(day => day.local_date === selected)
  function selectWithKeyboard(event: KeyboardEvent<SVGGElement>, index: number) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault()
      select(month.days[index].local_date)
    }
    if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
      event.preventDefault()
      const next = event.key === "Home" ? 0 : event.key === "End" ? month.days.length - 1 :
        Math.max(0, Math.min(month.days.length - 1, index + (event.key === "ArrowLeft" ? -1 : 1)))
      event.currentTarget.parentElement?.querySelectorAll<SVGGElement>('[role="button"]')[next]?.focus()
    }
  }
  return <article className="charging-month">
    <h3>{monthLabel(month.month)}</h3>
    <div className="charging-month-row">
      <div className="charging-chart">
        <svg viewBox="0 0 750 220" role="group" aria-label={`${monthLabel(month.month)} daily charging in ${unit === "cost" ? "estimated USD" : "kWh"}`}>
          {y.ticks(4).map(tick => <g key={tick} className="charging-grid" aria-hidden="true">
            <line x1="40" x2="740" y1={y(tick)} y2={y(tick)} />
            <text x="34" y={y(tick) + 4} textAnchor="end">{unit === "cost" ? "$" : ""}{y.tickFormat(4)(tick)}</text>
          </g>)}
          {month.days.map((day, index) => <g key={day.local_date} role="button" tabIndex={selected === day.local_date || !month.days.some(day => day.local_date === selected) && index === 0 ? 0 : -1}
            aria-label={dayLabel(day)} aria-pressed={selected === day.local_date}
            onMouseEnter={() => select(day.local_date)} onFocus={() => select(day.local_date)} onClick={() => select(day.local_date)}
            onKeyDown={event => selectWithKeyboard(event, index)} className={`charging-bar${day.future ? " charging-future" : ""}`}>
            <rect className="charging-hit" x={x(day.local_date)! - 2} y="10" width={x.bandwidth() + 4} height="185" />
            {layers.map(layer => {
              const point = layer[index]
              return <rect key={layer.key} className={`period-${layer.key}`} x={x(day.local_date)} y={y(point[1])} width={x.bandwidth()} height={Math.max(0, y(point[0]) - y(point[1]))} />
            })}
            {day.evidence.partial_sessions > 0 && <text x={x(day.local_date)! + x.bandwidth() / 2} y="9" textAnchor="middle" aria-hidden="true">*</text>}
            <text x={x(day.local_date)! + x.bandwidth() / 2} y="210" textAnchor="middle" aria-hidden="true">{index === 0 || (index + 1) % 5 === 0 || index === month.days.length - 1 ? index + 1 : ""}</text>
          </g>)}
        </svg>
        <label className="charging-day-selector">Day details
          <select aria-label={`Select day in ${monthLabel(month.month)}`} value={day?.local_date ?? ""} onChange={event => select(event.target.value)}>
            <option value="" disabled>Select a day</option>
            {month.days.map(day => <option key={day.local_date} value={day.local_date}>{dayLabel(day)}</option>)}
          </select>
        </label>
      </div>
      <dl className="charging-month-total">
        <div><dt>Estimated charging cost</dt><dd>{month.total?.display_cost ?? "No quoted charging"}</dd></div>
        <div><dt>Quoted vehicle-reported AC input</dt><dd>{month.total ? `${month.total.ac_kwh} kWh` : "No quoted energy"}</dd></div>
        {month.evidence.partial_sessions > 0 && <div><dt>Quality</dt><dd className="needs-review">Partial coverage *</dd></div>}
        {month.evidence.complete_zero_sessions > 0 && <div><dt>Evidence</dt><dd>Complete zero-energy evidence</dd></div>}
        {month.evidence.held_sessions > 0 && <div><dt>Excluded from chart</dt><dd>{month.evidence.held_sessions} held sessions</dd></div>}
        {month.evidence.other_sessions > 0 && <div><dt>Other chargers</dt><dd>{month.evidence.other_sessions} excluded sessions</dd></div>}
      </dl>
    </div>
    {day && <DayDetails day={day} />}
  </article>
}

export function MonthlyCharging({ history }: { history: MonthlyHistory }) {
  const [unit, setUnit] = useState<BarUnit>("cost")
  const [selected, setSelected] = useState("")
  return <section className="monthly-charging" aria-label="Monthly charging history">
    <div className="charging-toolbar">
      <h2>Monthly charging</h2>
      <div role="group" aria-label="Bar units">
        {(["cost", "kWh"] as const).map(option => <Button key={option} className="control" variant={unit === option ? "default" : "outline"} aria-pressed={unit === option} onClick={() => setUnit(option)}>{option === "cost" ? "Cost" : "kWh"}</Button>)}
      </div>
    </div>
    <div className="charging-legend">{chargingPeriods.map(period => <span key={period}><i className={`period-${period}`} />{periodLabel(period)}</span>)}</div>
    <p className="muted">Daily {unit === "cost" ? "estimated charging cost (USD)" : "quoted AC input (kWh)"} · {history.timezone}. Shaded days are in the future.</p>
    {history.evidence.held_sessions > 0 && <p className="muted charging-notice">{history.evidence.held_sessions} held (unpriceable) sessions are excluded from the chart.</p>}
    {history.undated_sessions > 0 && <p className="muted charging-notice">{history.undated_sessions} sessions have no available date and cannot be shown in a month.</p>}
    {history.months.map(month => <MonthChart key={month.month} month={month} unit={unit} selected={selected} select={setSelected} />)}
  </section>
}
