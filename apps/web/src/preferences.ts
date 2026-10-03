export const vehicleOptions = [
  { value: "all", label: "Both cars" },
  { value: "vehicle-a", label: "Joseph" },
  { value: "vehicle-b", label: "Evan" },
] as const

export type VehicleView = typeof vehicleOptions[number]["value"]
export type Preferences = { defaultVehicle: VehicleView; animateCharging: boolean }

const storageKey = "chargeshare.preview.preferences.v1"
const defaults: Preferences = { defaultVehicle: "all", animateCharging: true }

export function isVehicleView(value: unknown): value is VehicleView {
  return vehicleOptions.some((option) => option.value === value)
}

export function readPreferences(): Preferences {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "null")
    if (!stored || typeof stored !== "object") return { ...defaults }
    return {
      defaultVehicle: "defaultVehicle" in stored && isVehicleView(stored.defaultVehicle) ? stored.defaultVehicle : defaults.defaultVehicle,
      animateCharging: "animateCharging" in stored && typeof stored.animateCharging === "boolean" ? stored.animateCharging : defaults.animateCharging,
    }
  } catch {
    return { ...defaults }
  }
}

export function savePreferences(preferences: Preferences): boolean {
  try {
    localStorage.setItem(storageKey, JSON.stringify(preferences))
    return true
  } catch {
    return false
  }
}
