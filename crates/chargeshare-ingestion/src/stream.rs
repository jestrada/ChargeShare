use crate::IngestionError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

pub const SYNTHETIC_TOPIC: &str = "chargeshare_synthetic_V";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartitionStart {
    pub partition: i32,
    pub initial_offset: i64,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub epoch: String,
    pub topic: String,
    pub cluster_id: Option<String>,
    pub topic_id: Option<String>,
    pub partitions: Vec<PartitionStart>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PartitionRange {
    pub partition: i32,
    pub low: i64,
    pub high: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryCoordinate {
    pub partition: i32,
    pub offset: i64,
}

impl SourceBinding {
    pub fn validate(&self) -> Result<(), IngestionError> {
        let valid_alias = |value: &str| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        };
        if !self.epoch.starts_with("synthetic-")
            || !valid_alias(&self.epoch)
            || self.topic != SYNTHETIC_TOPIC
            || self.partitions.is_empty()
            || self.partitions.len() > 16
            || self
                .cluster_id
                .as_deref()
                .is_some_and(|id| !valid_alias(id))
            || self.topic_id.as_deref().is_some_and(|id| !valid_alias(id))
        {
            return Err(IngestionError::InvalidConfiguration);
        }
        let mut partitions = BTreeSet::new();
        for start in &self.partitions {
            if start.partition < 0
                || start.initial_offset < 0
                || !partitions.insert(start.partition)
            {
                return Err(IngestionError::InvalidConfiguration);
            }
        }
        Ok(())
    }

    pub fn verify_identity(&self, observed: &Self) -> Result<(), IngestionError> {
        self.validate()?;
        observed.validate()?;
        if self.epoch != observed.epoch
            || self.topic != observed.topic
            || self.cluster_id != observed.cluster_id
            || self.topic_id != observed.topic_id
            || self.partitions != observed.partitions
        {
            return Err(IngestionError::SourceMismatch);
        }
        Ok(())
    }

    pub fn validate_position(
        &self,
        partition: i32,
        next_offset: i64,
        bounds: &[PartitionRange],
    ) -> Result<(), IngestionError> {
        self.validate()?;
        if !self
            .partitions
            .iter()
            .any(|start| start.partition == partition)
        {
            return Err(IngestionError::SourceMismatch);
        }
        let mut matching = bounds.iter().filter(|bound| bound.partition == partition);
        let range = matching.next().ok_or(IngestionError::RecoveryGap)?;
        if matching.next().is_some()
            || range.low < 0
            || range.high < range.low
            || next_offset < range.low
            || next_offset > range.high
        {
            return Err(IngestionError::RecoveryGap);
        }
        Ok(())
    }
}

impl fmt::Debug for SourceBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceBinding")
            .field("partitions", &self.partitions.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> SourceBinding {
        SourceBinding {
            epoch: "synthetic-stream-1".to_owned(),
            topic: SYNTHETIC_TOPIC.to_owned(),
            cluster_id: Some("synthetic-cluster-1".to_owned()),
            topic_id: None,
            partitions: vec![PartitionStart {
                partition: 0,
                initial_offset: 5,
            }],
        }
    }

    #[test]
    fn source_configuration_requires_fictional_epoch_fixed_topic_and_unique_partitions() {
        let mut source = binding();
        assert_eq!(source.validate(), Ok(()));
        source.epoch.clear();
        assert_eq!(source.validate(), Err(IngestionError::InvalidConfiguration));
        source = binding();
        source.topic = "production-input".to_owned();
        assert_eq!(source.validate(), Err(IngestionError::InvalidConfiguration));
        source = binding();
        source.partitions.push(source.partitions[0].clone());
        assert_eq!(source.validate(), Err(IngestionError::InvalidConfiguration));
        source = binding();
        source.partitions[0].initial_offset = -1;
        assert_eq!(source.validate(), Err(IngestionError::InvalidConfiguration));
    }

    #[test]
    fn retained_bounds_accept_exact_edges_and_reject_missing_or_expired_progress() {
        let source = binding();
        let bounds = [PartitionRange {
            partition: 0,
            low: 5,
            high: 10,
        }];
        assert_eq!(source.validate_position(0, 5, &bounds), Ok(()));
        assert_eq!(source.validate_position(0, 10, &bounds), Ok(()));
        assert_eq!(
            source.validate_position(0, 4, &bounds),
            Err(IngestionError::RecoveryGap)
        );
        assert_eq!(
            source.validate_position(0, 11, &bounds),
            Err(IngestionError::RecoveryGap)
        );
        assert_eq!(
            source.validate_position(0, 5, &[]),
            Err(IngestionError::RecoveryGap)
        );
        assert_eq!(
            source.validate_position(1, 5, &bounds),
            Err(IngestionError::SourceMismatch)
        );
        assert_eq!(
            source.validate_position(0, 5, &[bounds[0], bounds[0]]),
            Err(IngestionError::RecoveryGap)
        );
    }

    #[test]
    fn epoch_and_cluster_changes_fail_without_payload_derived_diagnostics() {
        let source = binding();
        let mut changed = source.clone();
        changed.epoch = "synthetic-stream-2".to_owned();
        assert_eq!(
            source.verify_identity(&changed),
            Err(IngestionError::SourceMismatch)
        );
        changed = source.clone();
        changed.cluster_id = Some("synthetic-cluster-2".to_owned());
        assert_eq!(
            source.verify_identity(&changed),
            Err(IngestionError::SourceMismatch)
        );
        changed.epoch = "private-looking-marker".to_owned();
        assert!(!format!("{changed:?}").contains("private-looking-marker"));
    }
}
