mod error;

pub use error::IngestionError;
mod stream;

pub use stream::{
    DeliveryCoordinate, PartitionRange, PartitionStart, SYNTHETIC_TOPIC, SourceBinding,
};

mod rates;
pub use rates::{ChargerGeofenceConfig, OwnerChargerConfig, RateModelConfig, RatePeriodConfig};

mod live;
mod live_config;
mod manifest;
mod model;
mod normalize;

pub use live_config::{rebuild_live, run_live};

pub use manifest::{Annotation, DeviceMapping, Manifest, ManifestConfig, OwnerGeofence};
pub use model::{
    AcceptedEvidence, EvidenceProvenance, INPUT_CONTRACT_VERSION, MAX_ANNOTATIONS,
    MAX_MANIFEST_BYTES, MAX_MAPPINGS, MAX_PAYLOAD_BYTES, NORMALIZER_VERSION, NormalizedChargeType,
    NormalizedCounter, NormalizedEvent, NormalizedEventKind, NormalizedLocation, RejectionReason,
    SafeCounterProblem, TIMESTAMP_VERSION,
};
pub use normalize::normalize;
mod replay;
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
    CounterCoverageResult, LIVE_PRICING_BOUNDARY, LIVE_QUANTIZATION, PRICING_BOUNDARY,
    PricingResult, QuoteResult, ResultProvenance, ScopedResults, SessionPrice, SessionResult,
    SliceResult,
};
pub use viewing::serve_results;
