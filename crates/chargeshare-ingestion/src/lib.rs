mod error;

pub use error::IngestionError;
mod stream;

pub use stream::{
    DeliveryCoordinate, PartitionRange, PartitionStart, SYNTHETIC_TOPIC, SourceBinding,
};

mod manifest;
mod model;
mod normalize;

pub use manifest::{Annotation, DeviceMapping, Manifest, ManifestConfig};
pub use model::{
    AcceptedEvidence, EvidenceProvenance, INPUT_CONTRACT_VERSION, MAX_ANNOTATIONS,
    MAX_MANIFEST_BYTES, MAX_MAPPINGS, MAX_PAYLOAD_BYTES, NORMALIZER_VERSION, NormalizedChargeType,
    NormalizedCounter, NormalizedEvent, NormalizedEventKind, RejectionReason, SafeCounterProblem,
    TIMESTAMP_VERSION,
};
pub use normalize::normalize;
mod replay;
mod schema;
mod store;

pub use store::{CommitOutcome, PartitionPosition, RecordDisposition, Store, StoreStatistics};
mod kafka;

pub use kafka::{ConsumeReport, KafkaConfig, consume_bounded, read_source_marker};
