use crate::Result;
use crate::fixtures::ExpectedRecord;
use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TOPIC: &str = "chargeshare_synthetic_V";
const BROKER_DEADLINE_SECONDS: u64 = 25;

pub struct Broker {
    project_name: String,
}

impl Broker {
    pub fn new(project_name: &str) -> Self {
        Self {
            project_name: project_name.to_owned(),
        }
    }

    fn execute(&self, arguments: &[&str], stage: &str) -> Result<Vec<u8>> {
        let mut command = vec![
            "compose",
            "--project-name",
            &self.project_name,
            "--file",
            "dev/compose.yaml",
            "exec",
            "--no-TTY",
            "kafka",
        ];
        command.extend_from_slice(arguments);
        bounded_command("docker", &command, BROKER_DEADLINE_SECONDS, stage)
    }

    pub fn ready(&self) -> Result<()> {
        self.execute(
            &[
                "/opt/kafka/bin/kafka-broker-api-versions.sh",
                "--bootstrap-server",
                "kafka:9092",
            ],
            "broker protocol metadata",
        )?;
        self.execute(
            &[
                "/opt/kafka/bin/kafka-topics.sh",
                "--bootstrap-server",
                "kafka:9092",
                "--create",
                "--if-not-exists",
                "--topic",
                TOPIC,
                "--partitions",
                "1",
                "--replication-factor",
                "1",
            ],
            "synthetic topic setup",
        )?;
        Ok(())
    }

    pub fn offsets(&self) -> Result<BTreeMap<u32, u64>> {
        let output = self.execute(
            &[
                "/opt/kafka/bin/kafka-get-offsets.sh",
                "--bootstrap-server",
                "kafka:9092",
                "--topic",
                TOPIC,
                "--time",
                "-1",
            ],
            "broker starting offsets",
        )?;
        parse_offsets(&String::from_utf8(output)?)
    }

    pub fn observe(
        &self,
        offsets: &BTreeMap<u32, u64>,
        expected_count: usize,
    ) -> Result<Vec<ExpectedRecord>> {
        if offsets.len() != 1 || !offsets.contains_key(&0) {
            return Err(
                "run isolation: this pinned synthetic topic must have exactly partition 0".into(),
            );
        }
        let offset = offsets[&0].to_string();
        let count = expected_count.to_string();
        let output = self.execute(
            &[
                "/opt/kafka/bin/kafka-console-consumer.sh",
                "--bootstrap-server",
                "kafka:9092",
                "--topic",
                TOPIC,
                "--partition",
                "0",
                "--offset",
                &offset,
                "--max-messages",
                &count,
                "--timeout-ms",
                "20000",
                "--property",
                "print.key=true",
                "--property",
                "key.separator=\t",
            ],
            "Kafka observation: missing expected fixture output at 20-second deadline",
        )?;
        parse_records(&String::from_utf8(output)?)
    }
}

fn parse_offsets(output: &str) -> Result<BTreeMap<u32, u64>> {
    let mut offsets = BTreeMap::new();
    for line in output.lines() {
        let parts: Vec<_> = line.split(':').collect();
        if parts.len() != 3 || parts[0] != TOPIC {
            return Err("broker starting offsets: unexpected response".into());
        }
        if offsets
            .insert(parts[1].parse()?, parts[2].parse()?)
            .is_some()
        {
            return Err("broker starting offsets: duplicate partition".into());
        }
    }
    if offsets.is_empty() {
        return Err("broker starting offsets: topic missing; run readiness first".into());
    }
    Ok(offsets)
}

fn parse_records(output: &str) -> Result<Vec<ExpectedRecord>> {
    output
        .lines()
        .map(|line| {
            let (key, payload) = line
                .split_once('\t')
                .ok_or("Kafka decoding: expected key and JSON value")?;
            if !matches!(key, "device-1" | "device-2") {
                return Err("Kafka decoding: unexpected synthetic identity".into());
            }
            Ok(ExpectedRecord {
                key: key.to_owned(),
                payload: serde_json::from_str(payload)
                    .map_err(|_| "Kafka decoding: invalid receiver JSON")?,
            })
        })
        .collect()
}

pub fn bounded_command(
    program: &str,
    arguments: &[&str],
    timeout_seconds: u64,
    stage: &str,
) -> Result<Vec<u8>> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| format!("{stage}: required command is unavailable"))?;
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            if !output.status.success() {
                return Err(format!("{stage}: command failed").into());
            }
            return Ok(output.stdout);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err(format!("{stage}: {timeout_seconds}-second deadline exceeded").into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_offsets_are_per_partition_and_fail_closed() {
        assert_eq!(
            parse_offsets("chargeshare_synthetic_V:0:17\n").unwrap()[&0],
            17
        );
        assert!(parse_offsets("").is_err());
        assert!(parse_offsets("different:0:17").is_err());
        assert!(parse_offsets("chargeshare_synthetic_V:0:1\nchargeshare_synthetic_V:0:2").is_err());
    }

    #[test]
    fn decoded_record_requires_synthetic_key_and_valid_json() {
        assert_eq!(
            parse_records("device-1\t{\"vin\":\"device-1\"}\n")
                .unwrap()
                .len(),
            1
        );
        assert!(parse_records("device-unknown\t{}").is_err());
        assert!(parse_records("device-1\tinvalid").is_err());
        assert!(parse_records("{}").is_err());
    }

    #[test]
    fn command_failure_and_deadline_fail_the_stage() {
        assert!(bounded_command("sh", &["-c", "exit 3"], 1, "controlled failure").is_err());
        assert!(bounded_command("sleep", &["2"], 0, "controlled timeout").is_err());
        assert_eq!(
            bounded_command("printf", &["synthetic"], 1, "controlled success").unwrap(),
            b"synthetic"
        );
    }
}
