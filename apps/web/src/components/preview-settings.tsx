import { Button } from "@/components/ui/button"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { isVehicleView, vehicleOptions, type Preferences } from "@/preferences"

type PreviewSettingsProps = {
  preferences: Preferences
  onChange: (change: Partial<Preferences>) => void
  storageAvailable: boolean
}

export function PreviewSettings({ preferences, onChange, storageAvailable }: PreviewSettingsProps) {
  return (
    <Dialog>
      <DialogTrigger render={<Button variant="ghost" className="control" />}>Settings</DialogTrigger>
      <DialogContent className="settings-dialog" showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>Display preferences for this browser.</DialogDescription>
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
        <div className="settings-actions"><DialogClose render={<Button className="control" />}>Done</DialogClose></div>
      </DialogContent>
    </Dialog>
  )
}
