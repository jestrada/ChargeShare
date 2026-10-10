use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestionError {
    InvalidConfiguration,
    StorageUnavailable,
    StorageCorrupt,
    SchemaMismatch,
    ConfigurationMismatch,
    OwnershipConflict,
    SourceMismatch,
    RecoveryGap,
    ProgressConflict,
    DeliveryConflict,
    DomainReplay,
    Timeout,
    KafkaUnavailable,
    InvalidCommand,
}

impl fmt::Display for IngestionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfiguration => "configuration: invalid synthetic contract",
            Self::StorageUnavailable => "storage: unavailable or bounded write failed",
            Self::StorageCorrupt => "storage: invalid retained evidence",
            Self::SchemaMismatch => "storage: unsupported schema version",
            Self::ConfigurationMismatch => "storage: frozen configuration mismatch",
            Self::OwnershipConflict => "storage: another writer owns this runtime",
            Self::SourceMismatch => "source: broker runtime identity mismatch",
            Self::RecoveryGap => "source: durable position outside retained input",
            Self::ProgressConflict => "storage: unexpected partition progress",
            Self::DeliveryConflict => "storage: changed existing delivery contents",
            Self::DomainReplay => "replay: scoped evidence could not be reconstructed",
            Self::Timeout => "ingestion: bounded deadline exceeded",
            Self::KafkaUnavailable => "source: broker operation failed",
            Self::InvalidCommand => "command: invalid bounded synthetic invocation",
        })
    }
}

impl std::error::Error for IngestionError {}
