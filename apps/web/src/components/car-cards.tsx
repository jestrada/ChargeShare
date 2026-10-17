import { useEffect, useState, type CSSProperties } from "react"
import { carArtwork, readingAge, type CarScope } from "../persisted"

function CarArtwork({ color }: { color: string }) {
  const [unavailable, setUnavailable] = useState<string | null>(null)
  const privateImage = `/api/poc/car-images/${color.slice(1).toLowerCase()}.png`
  const artwork = carArtwork(color)
  if (unavailable !== privateImage) return <img className="car-artwork" src={privateImage} alt="" onError={() => setUnavailable(privateImage)} />
  if (artwork === "silhouette") return <span className="car-artwork car-silhouette" style={{ "--car-color": color } as CSSProperties} aria-hidden="true" />
  return <img className="car-artwork" src={`/cars/${artwork}.webp`} alt="" />
}

export function CarCards({ cars, selected, chooseScope }: { cars: CarScope[]; selected: string; chooseScope: (car: CarScope) => void }) {
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(tick)
  }, [])
  if (!cars.length) return null
  return <section className="car-cards" aria-label="Cars">
    {cars.map(car => {
      const active = `${car.owner}/${car.vehicle}` === selected
      return <button type="button" className="car-card glass" key={car.vehicle} aria-pressed={active} onClick={() => chooseScope(car)}>
        <span className="car-card-heading"><strong>{car.name}</strong><span className="car-selection">{active ? "Selected" : "Select car"}</span></span>
        <CarArtwork color={car.color} />
        <span className="car-battery">{car.battery_reading ? <>{Math.round(car.battery_reading.battery_percent)}% <span className="car-age">· {readingAge(car.battery_reading.observed_at, now)}</span></> : "Unavailable"}</span>
        <span className="car-status">{car.charging?.active && <span className="car-charging">Charging <span>· {readingAge(car.charging.observed_at, now)}</span></span>}</span>
      </button>
    })}
  </section>
}
