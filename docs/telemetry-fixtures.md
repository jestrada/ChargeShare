# Synthetic receiver fixtures and upstream provenance

The local harness exercises a finite synthetic mTLS WebSocket exchange through
Tesla's external Go receiver and its Kafka dispatcher. ChargeShare's outer driver
is Rust. The ledger, pricing core and fixture dashboard do not consume this
traffic. These fixtures contain only fictional `device-1` and `device-2`
identities, fixed counter/state values and fixed source timestamps. They are not
vehicle measurements, utility-meter evidence or reimbursement calculations.

## Immutable upstream inputs

`dev/upstream-pins.json` records the source revision, archive hashes, Linux amd64
image reference and native-library sources used by `dev/receiver.Dockerfile`.
The receiver source is unpatched at
[`bd076fe1494841707528449560c4a19d0d426da4`](https://github.com/teslamotors/fleet-telemetry/tree/bd076fe1494841707528449560c4a19d0d426da4).
Its committed `go.mod` and `go.sum` govern receiver dependencies; the image runs
`go mod download`, `go mod verify` and `go build -mod=readonly` with automatic Go
toolchain downloads disabled. Source and native archives use Docker's
`ADD --checksum` verification. There is no package-manager update, mutable image
base or copied private repository.

The build retains the upstream [Apache 2.0 license](licenses/fleet-telemetry-Apache-2.0.txt)
in the repository and resulting image. The pinned Go build image also serves as
the runtime image so its existing shell and curl can perform local readiness
checks. Compose selects the non-root process identity; runtime test certificates
are generated outside image layers and mounted read-only.

## Fixture contract

`dev/fixtures.json` contains `receiver_revision` and named `scenarios`. Each
scenario has finite `frames` and an expected Kafka semantic multiset:

- `frames`: `device`, binary `message_hex` and exact binary `acknowledgment_hex`
- `expected`: Kafka `key` and the complete decoded JSON `payload`

The sender replays the bytes rather than recreating Tesla's transport protocol.
The receiver determines identity from the authenticated certificate and writes
that identity into the decoded payload and Kafka key. The source fields remain
`VehicleName`, `BatteryLevel`, `DetailedChargeState` and `ACChargingEnergyIn`.
Battery and counter values are explicitly transmitted as strings; charging state
uses the upstream typed `DetailedChargeStateValue` enum. The counter input is a
synthetic cumulative AC-energy signal, not a computed session total.

| Scenario | Frames / expected V records | Intent |
| --- | --- | --- |
| `smoke` | 1 | First complete fixture record for `device-1`; minimal authenticated transport gate |
| `complete` | 6 | Three records each for two fictional vehicles |
| `missing` | 6 | The middle record for each vehicle omits `ACChargingEnergyIn` |
| `adverse` | 14 | Eight duplicate-case records followed by six out-of-order records |
| `all` | 26 | Complete, missing, duplicate and out-of-order sequences together |

Source moments are `2026-09-01T12:00:00Z`, `12:05:00Z` and `12:10:00Z`.
Complete and missing cases use that event-time order. The duplicate sequence uses
sample indexes `0, 0, 1, 2`, with a byte-identical repeated frame for each vehicle.
The out-of-order sequence uses sample indexes `2, 0, 1`. Vehicle one transmits
`100.00`, `100.25`, `100.50`; vehicle two transmits `200.00`, `200.125`, `200.25`.
Vehicle names are `Synthetic vehicle 1` and `Synthetic vehicle 2`; battery-level
strings are `50`, `51`, `52` for both vehicles. Each vehicle's typed state is
`DetailedChargeStateCharging`, `DetailedChargeStateCharging`,
`DetailedChargeStateComplete`. The missing case
omits the middle counter without substituting zero or inferring another value.
Each scenario embeds a fixed synthetic transaction identifier in its frames.

The selected receiver has no transport transaction/message deduplication gate:
each accepted duplicate frame is dispatched. Expectations therefore include both
copies. Per-vehicle semantic multisets preserve duplicate multiplicity and source
time while avoiding a global arrival-order promise. The verifier records Kafka
starting offsets before sending and isolates the run; retained older records
cannot satisfy a new acceptance run.

## Generation and regeneration recipe

Fixtures were generated once with official exported helpers from the pinned
source, using the official Go 1.27.1 Linux amd64 compiler archive. The compiler
archive's SHA-256 was verified against the
[official Go download manifest](https://go.dev/dl/?mode=json):
`63d339f0da5ab53635a56f2490a7984dfe12dfcff22ad749f63edaf590168445`.
No custom Go backend or Go runtime sender is committed to ChargeShare. Fixture
regeneration is a deliberate development operation, never an automatic startup
step or an opportunity to update the pinned source.

To regenerate in an isolated temporary checkout:

1. Fetch the exact receiver source archive and check its recorded SHA-256 before
   extracting it. Keep all generated work outside this repository until reviewed.
2. Use that source's committed generated protobuf package, `go.mod` and `go.sum`.
   Build each `protos.Payload` with an ordered `Data` slice, fixed `CreatedAt`,
   unset `Vin` and `IsResend=false`. Use the fields, values and sample orders above.
3. For each scenario/device/sample, set transaction ID to
   `synthetic-<original-case>-<device>-<sample-index>`, where original-case is
   `complete`, `missing-reading`, `duplicate` or `out-of-order`. Set message ID to
   that transaction ID followed by `-message`.
4. Marshal the payload with the official protobuf `proto.Marshal`. Invoke
   [`tesla.FlatbuffersStreamToBytes`](https://github.com/teslamotors/fleet-telemetry/blob/bd076fe1494841707528449560c4a19d0d426da4/messages/tesla/flatbuffers_extension.go)
   with sender ID `vehicle_device.<device>`, topic `V`, the transaction ID,
   protobuf payload, fixed source Unix seconds, fixed message ID, device type
   `vehicle_device`, device ID, and the same source Unix milliseconds as delivered
   time. Use the official `messages.StreamMessageFromBytes` to verify the result.
5. Generate expected acknowledgment bytes using the official
   `tesla.FlatbuffersStreamAckToBytes(transactionID, "V", nil)`. Parse them with
   `messages.StreamAckMessageFromBytes` and verify message type 5, topic `V`, the
   exact transaction ID and empty message ID.
6. Pass each encoded frame through the official
   [`telemetry.NewRecord`](https://github.com/teslamotors/fleet-telemetry/blob/bd076fe1494841707528449560c4a19d0d426da4/telemetry/record.go),
   using `NewBinarySerializer` with the corresponding synthetic `RequestIdentity`,
   `DeviceClientVersion="1.0.0"`, a `V` dispatch-rule entry and a no-op logger.
   Set `transmitDecodedRecords=true`. Save `record.Payload()` as the expected
   complete JSON and assert `record.Ack()` equals the generated acknowledgment.
7. Hex-encode the fixed frame and acknowledgment. Assemble the scenarios exactly
   as the table specifies; copy all expected payload fields and duplicate entries.
   Regenerate twice and compare the complete JSON and binary values before review.

The expected JSON uses upstream protobuf JSON options: symbolic enum names,
lower-camel field names and `EmitUnpopulated=true`. Thus `vin` is the authenticated
fictional identity, `createdAt` is the fixed source time and `isResend` is present
as `false`. The generation checks used the actual upstream record transforms,
not a separate handwritten imitation of decoded output. Two generation runs
produced identical artifacts. Actual receiver/Kafka execution is a separate
acceptance check; fixture generation alone does not establish transport success.

## Authentication, readiness and acknowledgment limits

The disposable CA uses common name `Tesla Motors Products CA`, which the pinned
upstream identity helper recognizes. This is a newly generated synthetic test CA,
not an imported Tesla certificate or credential. Client certificate common names
are `device-1` and `device-2`; the server common name and DNS subject alternative
name are `receiver`. `scripts/dev/certificates.sh` generates the disposable CA, server and client
certificates with the pinned shell's OpenSSL. Runtime paths are
`.local-runtime/certificates/{ca,server,client,client-device-2}.{crt,key}`. An
independent untrusted synthetic CA/client pair supports negative tests. Use
restricted runtime directories and `umask 077`; never publish the private files
or install them in a system trust store.

The client trusts only the generated local CA. The unpatched receiver retains
its selected upstream default CA bundle and adds the generated local CA. The
committed synthetic config selects the default production bundle with
`use_default_eng_ca=false`; it does not simultaneously select engineering trust.
This test-only environment is private and does not enroll or contact real
vehicles. Invalid-auth testing uses no client certificate or an independently
generated, untrusted synthetic CA.

The supported authenticated probe is HTTPS `/status` on transport port 4443 with
server verification and a generated client certificate: status 200 and body
`mtls ok`. The separate private HTTP `/status` on port 8080 returns `ok`, which
is process liveness only. Neither proves successful Kafka delivery. Synthetic
WebSocket requests use `/`, `Version: 1.0.0` and `X-Network-Interface: wifi`.
No insecure TLS flag or additional trust store is required.

`reliable_ack_sources: {"V": "kafka"}` delays the V acknowledgment until the
Kafka producer reports successful delivery. Kafka config sets `acks=all`; in a
single-broker local system that is an acknowledgment by the sole in-sync replica,
not high availability or guaranteed storage recovery. Connectivity events are
receiver-generated and do not use reliable acknowledgments. Decoded V records go
to `chargeshare_synthetic_V`; generated connectivity events go to
`chargeshare_synthetic_connectivity`.

The V acknowledgment contains its transaction ID, topic and an empty message ID.
It has no source/receipt time or connection ID, so exact fixture-byte comparison
is intentional. Kafka receipt metadata, offsets and connectivity envelope times
are volatile; source `payload.createdAt` and all transmitted field values are
stable. Upstream Kafka header `timestamp` remains `0` in this selected serializer,
so it must not be substituted for `payload.createdAt`.

A passing result requires both the exact protocol acknowledgment and the expected
new decoded Kafka records. ACK alone, a listening socket, direct Kafka fixture
production or successful fixture generation cannot pass that boundary. The test
makes no ledger deduplication, normalization, persistence, cost, production
reliability or receiver-backed dashboard claim.
