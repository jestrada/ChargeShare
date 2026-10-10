use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct Scenario {
    pub frames: Vec<Frame>,
    pub expected: Vec<ExpectedRecord>,
}

#[derive(Deserialize)]
pub struct Frame {
    pub device: String,
    pub message_hex: String,
    pub acknowledgment_hex: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExpectedRecord {
    pub key: String,
    pub payload: Value,
}

pub fn load(name: &str) -> Result<Scenario> {
    let fixtures: Value = serde_json::from_str(include_str!("../../../dev/fixtures.json"))?;
    let name = if name == "smoke" { "smoke" } else { name };
    serde_json::from_value(
        fixtures
            .get("scenarios")
            .and_then(|scenarios| scenarios.get(name))
            .ok_or("fixture: unknown scenario")?
            .clone(),
    )
    .map_err(Into::into)
}

pub fn decode_hex(encoded: &str) -> Result<Vec<u8>> {
    if !encoded.len().is_multiple_of(2) {
        return Err("fixture: malformed hex frame".into());
    }
    encoded
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let byte = std::str::from_utf8(pair)?;
            u8::from_str_radix(byte, 16).map_err(Into::into)
        })
        .collect()
}

pub fn normalized(records: &[ExpectedRecord]) -> Vec<Value> {
    let mut values: Vec<Value> = records
        .iter()
        .map(|record| json!({"key": record.key, "payload": record.payload}))
        .collect();
    values.sort_by_cached_key(Value::to_string);
    values
}

pub fn verify(expected: &[ExpectedRecord], observed: &[ExpectedRecord]) -> Result<()> {
    if normalized(expected) != normalized(observed) {
        return Err("record matching: decoded synthetic identity/source fields/values did not match the expected multiset".into());
    }
    Ok(())
}

pub fn missing_expectation() -> ExpectedRecord {
    ExpectedRecord {
        key: "device-missing-output".to_owned(),
        payload: json!({"vin": "device-missing-output"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_output_and_duplicate_loss_fail() {
        let record = missing_expectation();
        assert!(verify(std::slice::from_ref(&record), &[]).is_err());
        assert!(verify(&[record.clone(), record.clone()], &[record]).is_err());
    }

    #[test]
    fn source_times_and_vehicle_identity_remain_semantic() {
        let first = ExpectedRecord {
            key: "device-1".to_owned(),
            payload: json!({"createdAt":"2026-09-01T00:00:00Z","vin":"device-1"}),
        };
        let second = ExpectedRecord {
            key: "device-2".to_owned(),
            payload: json!({"createdAt":"2026-09-01T00:01:00Z","vin":"device-2"}),
        };
        assert!(verify(&[first.clone(), second.clone()], &[second, first]).is_ok());
        let mut changed = missing_expectation();
        changed.payload = json!({"createdAt":"2026-09-01T00:02:00Z"});
        assert!(verify(&[changed], &[]).is_err());
    }

    #[test]
    fn malformed_frames_are_rejected() {
        assert!(decode_hex("0").is_err());
        assert!(decode_hex("zz").is_err());
        assert_eq!(decode_hex("000aff").unwrap(), vec![0, 10, 255]);
    }
}
