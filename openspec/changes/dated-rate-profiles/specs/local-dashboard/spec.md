## MODIFIED Requirements

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
- **THEN** its rate version is visible and the off-peak rate displays as $0.2774 per kWh
- **AND** the quote still uses the exact 0.2773931020 rate

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

## ADDED Requirements

### Requirement: Sample reimbursement and vehicle context
The dashboard SHALL place Joseph’s eligible month subtotal payable to Evan first,
independent of the vehicle filter, with a sample label and visible held count.
Vehicle cards SHALL be compact on mobile and show labeled sample freshness and,
for the charging vehicle, sample kWh added. Session rows SHALL show TOU periods
and measured energy splits when available. Winter results SHALL say “Winter
estimate, unverified”.

#### Scenario: Reimbursement and uncertainty
- **WHEN** a sample is loaded or the vehicle filter changes
- **THEN** the top amount remains Joseph’s priced month subtotal, with any held sessions excluded and disclosed
- **AND** the September complete fixture shows $21.18 and never implies a payment was made

#### Scenario: Sample context on mobile
- **WHEN** viewed at 375px width
- **THEN** the reimbursement appears before the compact vehicle cards, which retain sample update times and charging energy
- **AND** sample scenario controls are in Settings

#### Scenario: TOU split
- **WHEN** the September 11 complete session is listed
- **THEN** the row shows 4 kWh off-peak and 6 kWh partial-peak
- **WHEN** its rate-boundary reading is missing
- **THEN** it shows both periods with an unresolved split, without inventing energy allocations

#### Scenario: October sample
- **WHEN** the October estimate scenario is selected in Settings
- **THEN** the same synthetic session shape is priced using the winter version and displays “Winter estimate, unverified”
