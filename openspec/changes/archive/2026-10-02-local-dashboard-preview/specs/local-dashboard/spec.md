# Local dashboard

## Purpose

Let contributors inspect shared charging and cost uncertainty in a local browser
using explicit fictional fixtures before any account or vehicle is connected.

## ADDED Requirements

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
Current and next sample prices SHALL come from the same fictional rate schedule
used for quotes and SHALL identify the fixed sample clock.

#### Scenario: Period and vehicle isolation
- **WHEN** the complete sample is shown for both vehicles
- **THEN** the September 7-13 week is 90 kWh and $20.40 and September 1-13 is 118 kWh and $26.00
- **AND** prior-month readings are excluded and vehicle filters scope both periods

#### Scenario: Held session affects relevant periods
- **WHEN** the missing-reading sample is loaded
- **THEN** the week is 80 kWh and $17.20 and month to date is 108 kWh and $22.80
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

### Requirement: Accessible menus and local preferences
The preview SHALL show Both cars, Joseph and Evan as three separate tab buttons
under Charging costs, with a white selected button and no shared segmented background, and
use styled shadcn selects for the default vehicle setting and scenario choices.
Help tooltips SHALL be accessible by hover, keyboard focus and a labeled tap target,
without hiding essential demo or review information. Settings SHALL contain only
a default vehicle view and charging animation preference saved in this browser.

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
