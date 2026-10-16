import { useId } from "react"
import { chargeEndTime, type CarScope } from "../persisted"

function CarArtwork({ color }: { color: string }) {
  const id = useId()
  return <svg className="car-artwork" viewBox="0 0 400 180" aria-hidden="true" focusable="false">
    <defs>
      <linearGradient id={`${id}-paint`} x1="0" y1="0" x2="0.2" y2="1">
        <stop stopColor="#fff" stopOpacity=".65" /><stop offset=".4" stopColor="#fff" stopOpacity=".08" /><stop offset=".7" stopColor="#000" stopOpacity=".12" /><stop offset="1" stopColor="#000" stopOpacity=".5" />
      </linearGradient>
      <linearGradient id={`${id}-glass`} x2=".6" y2="1">
        <stop stopColor="#58616e" /><stop offset="1" stopColor="#151c26" />
      </linearGradient>
      <radialGradient id={`${id}-wheel`}>
        <stop stopColor="#a5aeb9" /><stop offset=".35" stopColor="#4c5562" /><stop offset=".85" stopColor="#87919e" /><stop offset="1" stopColor="#343d49" />
      </radialGradient>
    </defs>
    <ellipse cx="201" cy="166" rx="180" ry="8" fill="#000" opacity=".18" />
    <path d="M25 113 28 104Q32 96 45 92L84 83C109 48 128 32 162 30C207 25 250 31 280 46C312 63 342 82 365 97Q376 103 378 117L378 134Q375 144 362 146H45Q27 145 25 132Z" fill={color} stroke="#66717e" strokeWidth="1.4" />
    <path d="M25 113 28 104Q32 96 45 92L84 83C109 48 128 32 162 30C207 25 250 31 280 46C312 63 342 82 365 97Q376 103 378 117L378 134Q375 144 362 146H45Q27 145 25 132Z" fill={`url(#${id}-paint)`} />
    <path d="M96 82C117 48 132 37 164 36C207 31 246 37 276 52L331 84Q240 90 96 88Z" fill={`url(#${id}-glass)`} stroke="#9099a4" strokeWidth="1" />
    <path d="M112 60C130 36 151 31 181 31C218 30 246 36 271 49" fill="none" stroke="#afc6db" strokeOpacity=".65" strokeWidth="2" />
    <path d="m180 35-2 53m86-43-7 42" stroke="#171e27" strokeWidth="6" />
    <path d="M40 96Q166 84 348 99M129 89l-7 46m112-47 9 47" fill="none" stroke="#fff" strokeOpacity=".28" />
    <path d="m33 135 337 1-9 11H45Z" fill="#18202a" />
    <path d="M50 147v-10a35 35 0 0 1 70 0v10m149 0v-10a35 35 0 0 1 70 0v10" fill="#171c24" />
    {[85, 304].map(x => <g key={x}>
      <circle cx={x} cy="137" r="29" fill="#131820" /><circle cx={x} cy="137" r="21" fill={`url(#${id}-wheel)`} />
      {[0, 72, 144, 216, 288].map(angle => <path key={angle} d={`M${x - 3} 137l-3-17h8l2 17Z`} fill="#b5bec8" stroke="#394452" strokeWidth="1" transform={`rotate(${angle} ${x} 137)`} />)}
      <circle cx={x} cy="137" r="5" fill="#596675" stroke="#a9b3bf" />
    </g>)}
    <path d="m33 100 31-7-5 7-28 6Z" fill="#ecf5ff" /><path d="m354 96 19 9 1 6-20-8Z" fill="#ca5652" />
    <path d="m141 96h15m88 0h15" stroke="#5f6975" strokeWidth="3" strokeLinecap="round" />
    <path d="m108 77-17 1-5 8 22-1Z" fill={color} stroke="#596471" />
    <path d="m28 121 20-3m316-11 3 22" stroke="#27313f" strokeWidth="2" strokeLinecap="round" />
  </svg>
}

export function CarCards({ cars, selected, timezone, chooseScope }: { cars: CarScope[]; selected: string; timezone: string | null; chooseScope: (car: CarScope) => void }) {
  if (!cars.length) return null
  return <section className="car-cards" aria-label="Cars">
    {cars.map(car => {
      const active = `${car.owner}/${car.vehicle}` === selected
      return <button type="button" className="car-card glass" key={car.vehicle} aria-pressed={active} onClick={() => chooseScope(car)}>
        <span className="car-card-heading"><strong>{car.name}</strong><span className="car-selection">{active ? "Selected" : "Select car"}</span></span>
        <CarArtwork color={car.color} />
        <span className="car-charge"><span className="car-battery">{car.last_charge?.battery_percent == null ? "Unavailable" : `${car.last_charge.battery_percent}%`}</span><span className="muted">at last charge end</span></span>
        <span className="car-charge-time">{car.last_charge ? chargeEndTime(car.last_charge.end_utc_seconds, timezone) : "No completed charge"}</span>
      </button>
    })}
  </section>
}
