import { useId, useState, type ReactNode } from "react"
import { InfoIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

export function HelpTooltip({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const triggerId = useId()
  const contentId = `${triggerId}-help`

  return (
    <Tooltip open={open} onOpenChange={setOpen} triggerId={triggerId}>
      <TooltipTrigger
        id={triggerId}
        aria-describedby={open ? contentId : undefined}
        closeOnClick={false}
        onClick={() => setOpen(true)}
        render={<Button variant="ghost" size="icon" className="help-trigger" aria-label={label} />}
      >
        <InfoIcon aria-hidden="true" />
      </TooltipTrigger>
      <TooltipContent id={contentId} role="tooltip" side="top" sideOffset={8} className="help-content">{children}</TooltipContent>
    </Tooltip>
  )
}
