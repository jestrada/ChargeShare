import { useState, type FormEvent } from "react"
import type { Scenario } from "@/demo"
import { Button } from "@/components/ui/button"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { isVehicleView, vehicleOptions, type Preferences } from "@/preferences"

type PreviewSettingsProps = {
  preferences: Preferences
  onChange: (change: Partial<Preferences>) => void
  storageAvailable: boolean
  scenario: Scenario
  onLoadScenario: (scenario: Scenario) => void
  loading: boolean
}

const scenarioOptions = [
  { value: "complete", label: "September · Complete readings" },
  { value: "missing", label: "September · Missing reading" },
  { value: "winter", label: "October · Winter estimate" },
] as const

export function PreviewSettings({ preferences, onChange, storageAvailable, scenario, onLoadScenario, loading }: PreviewSettingsProps) {
  const [selectedScenario, setSelectedScenario] = useState<Scenario>(scenario)

  function load(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    onLoadScenario(selectedScenario)
  }
  return (
    <Dialog>
      <DialogTrigger render={<Button variant="ghost" className="control" />}>Settings</DialogTrigger>
      <DialogContent className="settings-dialog" showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>Display preferences and sample data for this browser.</DialogDescription>
        </DialogHeader>
        <div className="settings-field">
          <label htmlFor="default-vehicle">Default vehicle view</label>
          <Select items={vehicleOptions} value={preferences.defaultVehicle} onValueChange={(value) => {
            if (isVehicleView(value)) onChange({ defaultVehicle: value })
          }}>
            <SelectTrigger id="default-vehicle" aria-describedby="default-vehicle-note"><SelectValue /></SelectTrigger>
            <SelectContent align="start" alignItemWithTrigger={false} sideOffset={8}>
              <SelectGroup>{vehicleOptions.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectGroup>
            </SelectContent>
          </Select>
          <p id="default-vehicle-note" className="muted">The cost view shown when you open ChargeShare.</p>
        </div>
        <label className="settings-motion" htmlFor="animate-charging">
          <span><span>Animate charging</span><span className="muted">Follows your device’s reduced-motion preference.</span></span>
          <Switch id="animate-charging" checked={preferences.animateCharging} onCheckedChange={(checked) => onChange({ animateCharging: checked })} />
        </label>
        <p className="muted settings-save-note" role="status">{storageAvailable ? "Changes save automatically in this browser." : "Changes apply for this visit. Your browser couldn’t save them."}</p>
        <form className="settings-field settings-scenario" onSubmit={load}>
          <label htmlFor="scenario">Sample data</label>
          <Select items={scenarioOptions} value={selectedScenario} onValueChange={(value) => {
            if (value === "complete" || value === "missing" || value === "winter") setSelectedScenario(value)
          }}>
            <SelectTrigger id="scenario"><SelectValue /></SelectTrigger>
            <SelectContent align="start" alignItemWithTrigger={false} sideOffset={8}>
              <SelectGroup>{scenarioOptions.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectGroup>
            </SelectContent>
          </Select>
          <p className="muted" role="status">{loading ? "Loading sample…" : `Showing ${scenarioOptions.find((option) => option.value === scenario)?.label.toLowerCase()}.`}</p>
          <Button className="control" type="submit" disabled={loading}>Load scenario</Button>
        </form>
        <div className="settings-actions"><DialogClose render={<Button className="control" />}>Done</DialogClose></div>
      </DialogContent>
    </Dialog>
  )
}
