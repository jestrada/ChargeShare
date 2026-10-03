export type Scenario = "complete" | "missing" | "winter"

export type CostLine = {
  energy: string
  rate: string
  cost: string
  version: string
}

export type Session = {
  id: string
  date: string
  date_order: number
  time: string
  vehicle: string
  energy: string
  cost: string | null
  reason: string | null
  lines: CostLine[]
  periods: { label: string; energy: string | null }[]
}

export type Totals = {
  priced_energy: string
  priced_subtotal: string
  held: number
}

export type PeriodTotals = { week: Totals; month: Totals }

export type Vehicle = PeriodTotals & {
  id: string
  name: string
  model: string
  battery: number
  state: string
  sessions: Session[]
  last_updated: string
  added_kwh: string | null
}

export type DemoSnapshot = {
  scenario: Scenario
  vehicles: Vehicle[]
  totals: PeriodTotals
  reimbursement: Totals
  periods: { week: string; month: string; as_of: string; name: string }
  rates: {
    label: string
    current: { price: string; until: string } | null
    next: { price: string; starts: string; ends: string } | null
  }
}
