mod error;

pub use error::IngestionError;
mod stream;

pub use stream::{
    DeliveryCoordinate, PartitionRange, PartitionStart, SYNTHETIC_TOPIC, SourceBinding,
};

mod charger_rates;
mod rates;
pub use rates::{ChargerGeofenceConfig, OwnerChargerConfig, RateModelConfig, RatePeriodConfig};

mod live;
mod manifest;
mod model;
mod normalize;
mod registry;
mod registry_bootstrap;
mod registry_recovery;
mod runtime_settings;

pub use registry::{CarRegistration, RegistrySetup, RegistryWrite, migrate_registry};
pub use registry_bootstrap::{RegistryBootstrap, rebuild_live};
pub use runtime_settings::{RuntimeSettings, run_live};

pub use manifest::{Annotation, DeviceMapping, Manifest, ManifestConfig, OwnerGeofence};
pub use model::{
    AcceptedEvidence, BatteryPercent, EvidenceProvenance, INPUT_CONTRACT_VERSION, MAX_ANNOTATIONS,
    MAX_MANIFEST_BYTES, MAX_MAPPINGS, MAX_PAYLOAD_BYTES, NORMALIZER_VERSION, NormalizedChargeType,
    NormalizedCounter, NormalizedEvent, NormalizedEventKind, NormalizedLocation, RejectionReason,
    SafeCounterProblem, TIMESTAMP_VERSION,
};
pub use normalize::normalize;
mod replay;
mod vehicle_status;
pub use vehicle_status::{BatteryReading, ChargingObservation};
mod schema;
mod store;

pub use store::{
    CommitBoundary, CommitOutcome, PartitionPosition, RecordDisposition, Store, StoreStatistics,
};
mod kafka;

pub use kafka::{
    ConsumeReport, KafkaConfig, RereadRange, RereadReport, consume_bounded,
    consume_bounded_observed, read_source_marker, reread_bounded,
};

mod results;
mod viewing;

pub use results::{
    ChargerSummary, CounterCoverageResult, LIVE_PRICING_BOUNDARY, LIVE_QUANTIZATION,
    PRICING_BOUNDARY, PricingResult, QuoteResult, ResultProvenance, ResultRegistry, ScopedResults,
    SessionChargerResult, SessionLocationVersion, SessionPrice, SessionResult, SliceResult,
};
pub use viewing::serve_results;
