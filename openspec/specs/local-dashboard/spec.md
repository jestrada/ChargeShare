# local-dashboard Specification

## Purpose

Let contributors inspect shared charging and cost uncertainty in a local browser
using explicit fictional fixtures before any account or vehicle is connected.

## Requirements

### Requirement: Local synthetic dashboard
The preview SHALL bind only to loopback and display a persistent demo-data label.
Battery percentages and statuses SHALL be identified as sample values. Session
energy and costs MUST come from the core ledger/pricing engine.

#### Scenario: Open the preview
- **WHEN** a user opens the documented localhost URL
- **THEN** a responsive near-black dashboard with a regular-weight Geist ChargeShare title, no separate wordmark bar or theme selector shows two sample vehicles, energy, priced subtotals and session details
- **AND** no account, external asset, live vehicle or private file is accessed

### Requirement: Meaningful review interactions
The preview SHALL support complete and missing-reading scenarios, vehicle filters,
and expandable cost details. Held costs SHALL appear as needing review rather
than as zero owed. Switching scenarios SHALL not mutate fixtures or core state.

#### Scenario: Missing reading
- **WHEN** the missing-reading scenario is selected
- **THEN** the affected session is held with a plain-language explanation and omitted from the priced subtotal
- **WHEN** complete readings are selected again
- **THEN** the original deterministic priced results return

#### Scenario: Filter a vehicle
- **WHEN** one vehicle is selected
- **THEN** the session list and priced subtotals include only that vehicle
- **AND** opening a priced session shows its energy, rates and cost breakdown

#### Scenario: Read session times
- **WHEN** a session is listed, including one needing review
- **THEN** its elapsed session duration and start/stop times are visible without expanding the row, such as `2hr (1:00am - 3:00am)`
- **AND** the times remain readable beside its date and owner at mobile width
- **AND** desktop rows align session, duration, energy and cost under matching column labels, while mobile rows retain a compact two-line layout
- **AND** a down caret indicates that a row can expand and points up when open

### Requirement: Bounded preview surface
The preview API SHALL reject unknown routes and unsupported methods without accepting
uploads, credentials or user-entered records. It SHALL not expose a live control,
authentication, backend persistence or deployment feature. Browser storage SHALL
be limited to the display preferences specified below.

#### Scenario: Unsupported input
- **WHEN** an unknown route, query value or non-GET method is requested
- **THEN** the server returns a safe error and does not reflect arbitrary input

#### Scenario: Mobile and keyboard use
- **WHEN** the preview is viewed on a narrow screen or navigated with a keyboard
- **THEN** scenario selection, vehicle filters and cost details remain accessible

### Requirement: Bounded sample totals and rate outlook
The preview SHALL show weekly and month-to-date priced costs/energy with explicit
sample dates, calculated by summing exact eligible core quotes before rounding.
Held sessions MUST be counted and excluded from both totals as appropriate.
Current and next sample prices SHALL come from the same dated sample rate schedule
used for quotes and SHALL identify the fixed sample clock. Period/date labels SHALL
be derived from fixture clock/boundaries. Rates SHALL display four decimal places
while costs retain exact input precision; priced session details SHALL name their
rate version. The default SHALL use the September sample public tariff.

#### Scenario: Period and vehicle isolation
- **WHEN** the complete sample is shown for both vehicles
- **THEN** the September 7-13 week is 90 kWh and $27.38 and September 1-13 is 118 kWh and $35.14
- **AND** prior-month readings are excluded and vehicle filters scope both periods

#### Scenario: Held session affects relevant periods
- **WHEN** the missing-reading sample is loaded
- **THEN** the week is 80 kWh and $23.40 and month to date is 108 kWh and $31.16
- **AND** the affected session remains visible with a reason and both held counts are one

#### Scenario: Rate transition
- **WHEN** the sample clock is before a rate boundary
- **THEN** the active rate and the immediately following window are shown
- **WHEN** the lookup is evaluated at the boundary
- **THEN** the new rate is active with no overlap or stale previous rate

#### Scenario: Charging motion preference
- **WHEN** a sample vehicle is charging
- **THEN** green fill/glow and a text charging label communicate its state
- **AND** reduced-motion preference disables its animation without hiding its state

#### Scenario: Ambient charging effect
- **WHEN** a sample vehicle is charging with animation enabled
- **THEN** a subtle green border glow breathes and faint overhead rays move behind the card content
- **AND** the decorative effects do not intercept pointer events, alter readings or appear on unplugged vehicles
- **WHEN** charging animation is disabled or reduced motion is requested
- **THEN** the halo and rays remain still and the charging text stays visible
- **WHEN** reduced transparency is requested
- **THEN** the rays are hidden and the charging text and fill remain readable

#### Scenario: Rate provenance and precision
- **WHEN** a priced session is expanded
- **THEN** its rate version IDs are visible and each cost line shows energy, a four-decimal USD/kWh rate and cost
- **AND** the September off-peak rate displays as $0.2774 per kWh
- **AND** the quote still uses the exact 0.2773931020 rate

#### Scenario: Fixture-derived labels
- **WHEN** the fixture clock or period boundaries change
- **THEN** week, month and sample-clock labels follow those values rather than retaining September's hardcoded dates

#### Scenario: Unavailable rate outlook
- **WHEN** the sample clock has no active rate or immediately adjacent next window
- **THEN** the corresponding price says "Rate unavailable" without filling the gap from a nearby window

### Requirement: Accessible menus and local preferences
The preview SHALL show Both cars, Joseph and Evan as three separate tab buttons
under Charging costs, with a white selected button and no shared segmented background, and
use styled shadcn selects for the default vehicle setting and scenario choices.
Help tooltips SHALL be accessible by hover, keyboard focus and a labeled tap target,
without hiding essential demo or review information. Settings SHALL contain the sample scenario picker, a default vehicle view and
charging animation preference. Only display preferences SHALL persist in this
browser; a fresh browser SHALL default to Joseph.

#### Scenario: Select and dismiss
- **WHEN** a user opens a selector with pointer or keyboard
- **THEN** the selected item is indicated, arrow keys and Enter can change it, and Escape dismisses the popup with focus returned to its trigger
- **AND** menus stay inside narrow viewports and their rows have at least 40px tap targets

#### Scenario: Switch vehicle tabs
- **WHEN** a user taps a vehicle tab or navigates it with arrow keys
- **THEN** both period totals and the session list update to that vehicle in an associated tab panel
- **AND** all three choices remain visible at mobile width with at least 40px tap targets

#### Scenario: Save display preferences
- **WHEN** a user changes the default vehicle view or charging animation in Settings
- **THEN** the preference applies immediately and survives a reload in this browser
- **AND** changing the ordinary vehicle filter does not rewrite the saved default
- **AND** these preferences do not imply authentication or a Tesla connection

#### Scenario: Storage and motion fallback
- **WHEN** stored preferences are invalid or browser storage is unavailable
- **THEN** the dashboard remains usable with safe defaults or in-memory preferences
- **WHEN** reduced motion is requested by the operating system or animation is disabled
- **THEN** the charging label and battery fill remain visible without charging animation

#### Scenario: Initial vehicle view
- **WHEN** a browser has no saved default
- **THEN** Joseph's tab, period totals and sessions are selected on load
- **WHEN** a valid default vehicle preference has already been saved
- **THEN** that preference determines the initial view instead

#### Scenario: Sample controls in Settings
- **WHEN** the dashboard is displayed with Settings closed
- **THEN** the sample scenario selector and Load scenario action are absent from the main page
- **WHEN** Settings is opened and a scenario is selected and loaded
- **THEN** the dashboard uses that sample without changing saved display preferences
- **AND** a page reload returns to the default September complete sample

### Requirement: Sample reimbursement
The dashboard SHALL place Joseph’s eligible month subtotal payable to Evan first,
independent of the vehicle filter, with a sample label. It SHALL state that the
amount is energy only and excludes fixed charges and credits. Held sessions SHALL
be excluded and counted when present; an all-held amount SHALL say "Needs review".
Winter results SHALL say “Winter estimate, unverified”.

#### Scenario: Reimbursement and uncertainty
- **WHEN** a sample is loaded or the vehicle filter changes
- **THEN** the top amount remains Joseph’s priced month subtotal, with any held sessions excluded and disclosed
- **AND** the September complete fixture shows $21.18 and never implies a payment was made

#### Scenario: Energy-only amount
- **WHEN** a sample reimbursement is shown
- **THEN** the card states "Energy only. Fixed charges and credits not included."
- **AND** the session footer also discloses those exclusions

#### Scenario: All reimbursement sessions held
- **WHEN** Joseph has held sessions and no priced energy in the sample month
- **THEN** the card shows "Needs review" instead of a zero owed amount
- **AND** it shows the excluded-session count and says the amount is incomplete

#### Scenario: October sample
- **WHEN** the October estimate scenario is selected in Settings
- **THEN** the same synthetic session shape is priced using the winter version and displays “Winter estimate, unverified”

### Requirement: Sample vehicle context
Vehicle cards SHALL be compact on mobile and show a last-updated fixture time.
The charging vehicle SHALL also show fixture kWh added so far. Both lines SHALL
be explicitly labeled sample values and SHALL not imply live telemetry.

#### Scenario: Sample context on mobile
- **WHEN** viewed at 375px width
- **THEN** the reimbursement appears before the compact vehicle cards, which retain sample update times and charging energy

#### Scenario: Fixture freshness and charging energy
- **WHEN** sample vehicle cards are shown
- **THEN** each last-updated line is marked "(sample)"
- **AND** the charging vehicle shows "6 kWh added so far (sample)" while the unplugged vehicle has no added-energy line

### Requirement: Session time-of-use context
Session rows SHALL show their TOU periods inline and measured energy splits when
available. Unresolved splits SHALL identify the periods without inventing energy
allocations; absent coverage SHALL say "Rate unavailable".

#### Scenario: TOU split
- **WHEN** the September 11 complete session is listed
- **THEN** the row shows 4 kWh off-peak and 6 kWh partial-peak
- **WHEN** its rate-boundary reading is missing
- **THEN** it shows both periods with an unresolved split, without inventing energy allocations

#### Scenario: No covered period
- **WHEN** a session has no applicable rate windows
- **THEN** its row says "Rate unavailable" and the cost remains "Needs review"
