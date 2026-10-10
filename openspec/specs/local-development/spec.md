# local-development Specification

## Purpose

Provide a repeatable, inspectable development environment for synthetic ChargeShare work on Linux x86_64 machines, including suitable cloud Linux environments, without requiring cloud provisioning or a live vehicle connection.

## Requirements

### Requirement: Reproducible developer entry point
The environment SHALL provide `nix develop` followed by `tilt up` as its normal startup path on Linux x86_64 after documented host prerequisites. Suitable cloud Linux environments SHALL meet the same permitted Docker-runtime and private-networking prerequisites. Tool and upstream service inputs MUST be pinned and version-inspectable. Startup MUST NOT update lockfiles or require Kubernetes, Tesla credentials or new cloud accounts. Support for other operating systems and architectures is outside this change.

#### Scenario: Fresh supported checkout
- **WHEN** a developer follows the prerequisite instructions on a tested Linux x86_64 configuration, on a machine or in a suitable cloud environment, and starts a clean checkout
- **THEN** the pinned tools are available and local resources start without manually launching each service
- **AND** required dependency installation and first-run downloads are documented and visible
- **AND** cloud execution preserves private container networking and loopback host listeners without provisioning public ingress or deployment

#### Scenario: Missing host prerequisite
- **WHEN** the Docker daemon is stopped, inaccessible or configured for an unsupported container platform
- **THEN** startup reports the specific prerequisite and corrective step without attempting privileged host changes or reporting readiness

### Requirement: Observable service lifecycle
The environment SHALL expose per-resource status and logs for the broker, receiver, synthetic harness and existing preview. Readiness MUST check each resource's usable interface with bounded timeouts. A running process alone MUST NOT count as a successful receiver-to-broker test.

#### Scenario: Ready environment
- **WHEN** the broker accepts protocol requests, receiver accepts the synthetic authenticated transport, and preview responds successfully
- **THEN** the corresponding resources show ready and their local entry points and logs are discoverable
- **AND** harness verification remains a separate explicit result

#### Scenario: Port conflict or unhealthy dependency
- **WHEN** a required local port is occupied or a resource cannot become ready before its documented timeout
- **THEN** the affected resource shows failure with a safe reason and a recovery step
- **AND** dependent verification cannot report success

### Requirement: Explicit shutdown and isolated reset
The documented shutdown SHALL stop this environment's services and release its listeners without deleting retained synthetic state. A separate explicit reset SHALL affect only this checkout's local runtime state and SHALL support reproducible clean reruns. Merely closing the Tilt UI or shell MUST NOT be documented as shutting down all services.

#### Scenario: Stop and restart
- **WHEN** the developer runs the documented shutdown and then starts again
- **THEN** no managed listeners remain after shutdown and retained synthetic broker records are available after restart

#### Scenario: Expired or incomplete test trust
- **WHEN** generated test certificates have expired or their bundle is incomplete
- **THEN** documented shutdown still stops the owned environment without preparing certificates
- **AND** configuration and runtime ownership checks remain enforced
- **AND** startup refuses invalid trust until the stopped environment is explicitly reset

#### Scenario: Clean rerun
- **WHEN** the developer explicitly requests the documented reset while the environment is stopped
- **THEN** only the named project runtime files, broker volume and harness consumer state are removed or recreated
- **AND** source files, other checkouts, unrelated Docker resources and browser preferences remain untouched
- **AND** attempting reset while resources are active fails with a stop-first instruction

### Requirement: Local synthetic privacy boundary
The default environment MUST publish host ports only on loopback and use only generated local-test credentials and fictional inputs. Generated keys, certificates, logs and broker state MUST remain untracked runtime data. It MUST NOT install trust roots, expose public tunnels, load real vehicle configuration or contact Tesla APIs.

#### Scenario: Inspect local surfaces and files
- **WHEN** the environment is started with defaults
- **THEN** only required loopback host listeners and private inter-container endpoints exist
- **AND** generated private material is excluded from version control, public artifacts and the Nix store

#### Scenario: Unsafe local configuration
- **WHEN** startup detects non-loopback host publishing or attempts to select a non-synthetic receiver configuration
- **THEN** it fails with a safe configuration error instead of silently widening access

### Requirement: Honest preview integration boundary
The existing dashboard SHALL retain its demo labels, fixture data and established loopback behavior when managed by the local environment. Environment documentation and status MUST state that receiver traffic is not yet connected to the ledger, persistence or dashboard.

#### Scenario: Inspect demo alongside telemetry test
- **WHEN** a synthetic receiver scenario succeeds while the dashboard is open
- **THEN** the result is labeled receiver-to-Kafka verification only
- **AND** existing dashboard totals remain their fixture-derived values rather than implying receiver-backed ingestion
