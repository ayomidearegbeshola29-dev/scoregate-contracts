//! Replay determinism checks across host versions.
//! Identifies semantic divergences in behavior between different Soroban host versions.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// Determinism check result
pub type DeterminismResult<T> = Result<T, DeterminismError>;

/// Determinism-related errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeterminismError {
    /// State divergence detected
    StateDivergence {
        host_version_a: String,
        host_version_b: String,
        key: String,
        value_a: String,
        value_b: String,
    },
    /// Event divergence detected
    EventDivergence {
        host_version_a: String,
        host_version_b: String,
        index: usize,
        event_a: String,
        event_b: String,
    },
    /// Error code divergence
    ErrorCodeDivergence {
        host_version_a: String,
        host_version_b: String,
        error_a: u32,
        error_b: u32,
    },
    /// Execution divergence (output differs)
    ExecutionDivergence {
        host_version_a: String,
        host_version_b: String,
        reason: String,
    },
    /// Invalid comparison parameters
    InvalidComparison { reason: String },
}

impl std::fmt::Display for DeterminismError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeterminismError::StateDivergence { host_version_a, host_version_b, key, value_a, value_b } => {
                write!(
                    f,
                    "State divergence: key '{}' differs between {} and {}. Value in {}: '{}', in {}: '{}'",
                    key, host_version_a, host_version_b, host_version_a, value_a, host_version_b, value_b
                )
            }
            DeterminismError::EventDivergence { host_version_a, host_version_b, index, event_a, event_b } => {
                write!(
                    f,
                    "Event divergence: event #{} differs between {} and {}. Event in {}: '{}', in {}: '{}'",
                    index, host_version_a, host_version_b, host_version_a, event_a, host_version_b, event_b
                )
            }
            DeterminismError::ErrorCodeDivergence { host_version_a, host_version_b, error_a, error_b } => {
                write!(
                    f,
                    "Error code divergence: {} produced error {}, {} produced error {}",
                    host_version_a, error_a, host_version_b, error_b
                )
            }
            DeterminismError::ExecutionDivergence { host_version_a, host_version_b, reason } => {
                write!(f, "Execution divergence between {} and {}: {}", host_version_a, host_version_b, reason)
            }
            DeterminismError::InvalidComparison { reason } => {
                write!(f, "Invalid comparison: {}", reason)
            }
        }
    }
}

impl std::error::Error for DeterminismError {}

/// Execution result for a given input and host version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostVersionResult {
    /// Soroban host version
    pub host_version: String,
    /// Accepted entries count
    pub accepted_count: u32,
    /// Rejected entries count
    pub rejected_count: u32,
    /// State snapshot (key-value pairs)
    pub state_snapshot: HashMap<String, String>,
    /// Events emitted during execution
    pub events: Vec<String>,
    /// Error code if execution failed
    pub error_code: Option<u32>,
    /// Execution metadata
    pub metadata: ExecutionMetadata,
}

/// Execution metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetadata {
    /// Total gas consumed
    pub gas_consumed: Option<u64>,
    /// Soroban CPU instructions consumed
    #[serde(default)]
    pub cpu_instructions: Option<u64>,
    /// Soroban RAM bytes consumed
    #[serde(default)]
    pub memory_bytes: Option<u64>,
    /// Execution time in milliseconds
    pub execution_time_ms: Option<u64>,
    /// Peak memory usage in bytes
    pub peak_memory_bytes: Option<u64>,
    /// Custom metadata
    #[serde(default)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Determinism comparison result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterminismComparison {
    /// First host version tested
    pub host_version_a: String,
    /// Second host version tested
    pub host_version_b: String,
    /// Whether results are deterministic
    pub is_deterministic: bool,
    /// Any divergences found
    pub divergences: Vec<DeterminismError>,
    /// State differences
    pub state_differences: Vec<StateDifference>,
    /// Event differences
    pub event_differences: Vec<EventDifference>,
}

/// Single state difference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDifference {
    /// State key that differs
    pub key: String,
    /// Value in first host version
    pub value_a: Option<String>,
    /// Value in second host version
    pub value_b: Option<String>,
}

/// Single event difference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDifference {
    /// Event index
    pub index: usize,
    /// Event in first host version
    pub event_a: Option<String>,
    /// Event in second host version
    pub event_b: Option<String>,
}

/// Replay scenario for determinism testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayScenario {
    /// Scenario name/identifier
    pub name: String,
    /// Description of what's being tested
    pub description: Option<String>,
    /// Input entries
    pub entries: Vec<serde_json::Value>,
    /// Expected deterministic behavior documentation
    pub expected_behavior: Option<String>,
}

/// Compare two execution results for determinism
pub fn compare_results(
    result_a: &HostVersionResult,
    result_b: &HostVersionResult,
) -> DeterminismComparison {
    let mut divergences = Vec::new();
    let mut state_differences = Vec::new();
    let mut event_differences = Vec::new();

    // Check acceptance/rejection counts
    if result_a.accepted_count != result_b.accepted_count
        || result_a.rejected_count != result_b.rejected_count
    {
        divergences.push(DeterminismError::ExecutionDivergence {
            host_version_a: result_a.host_version.clone(),
            host_version_b: result_b.host_version.clone(),
            reason: format!(
                "Acceptance counts differ: ({}, {}) vs ({}, {})",
                result_a.accepted_count,
                result_a.rejected_count,
                result_b.accepted_count,
                result_b.rejected_count
            ),
        });
    }

    // Check error codes
    if result_a.error_code != result_b.error_code {
        if let (Some(ea), Some(eb)) = (result_a.error_code, result_b.error_code) {
            divergences.push(DeterminismError::ErrorCodeDivergence {
                host_version_a: result_a.host_version.clone(),
                host_version_b: result_b.host_version.clone(),
                error_a: ea,
                error_b: eb,
            });
        } else {
            divergences.push(DeterminismError::ExecutionDivergence {
                host_version_a: result_a.host_version.clone(),
                host_version_b: result_b.host_version.clone(),
                reason: format!("One version failed, the other succeeded"),
            });
        }
    }

    for (label, value_a, value_b) in [
        (
            "gas consumed",
            result_a.metadata.gas_consumed,
            result_b.metadata.gas_consumed,
        ),
        (
            "CPU instructions",
            result_a.metadata.cpu_instructions,
            result_b.metadata.cpu_instructions,
        ),
        (
            "memory bytes",
            result_a.metadata.memory_bytes,
            result_b.metadata.memory_bytes,
        ),
    ] {
        match (value_a, value_b) {
            (Some(value_a), Some(value_b)) => {
                if value_a != value_b {
                    divergences.push(DeterminismError::ExecutionDivergence {
                        host_version_a: result_a.host_version.clone(),
                        host_version_b: result_b.host_version.clone(),
                        reason: format!(
                            "{} differs: {} vs {}",
                            label, value_a, value_b
                        ),
                    });
                }
            }
            (None, None) => {}
            (value_a, value_b) => {
                divergences.push(DeterminismError::ExecutionDivergence {
                    host_version_a: result_a.host_version.clone(),
                    host_version_b: result_b.host_version.clone(),
                    reason: format!("{} differs: {} vs {}", label, value_a, value_b),
                });
            }
        }
    }

    // Compare state snapshots using ordered key iteration so that diagnostic
    // ordering is stable regardless of HashMap insertion order.
    let mut all_keys: BTreeSet<&String> = BTreeSet::new();
    all_keys.extend(result_a.state_snapshot.keys());
    all_keys.extend(result_b.state_snapshot.keys());

    for key in all_keys {
        let value_a = result_a.state_snapshot.get(key);
        let value_b = result_b.state_snapshot.get(key);

        if value_a != value_b {
            state_differences.push(StateDifference {
                key: key.clone(),
                value_a: value_a.cloned(),
                value_b: value_b.cloned(),
            });

            if let (Some(va), Some(vb)) = (value_a, value_b) {
                divergences.push(DeterminismError::StateDivergence {
                    host_version_a: result_a.host_version.clone(),
                    host_version_b: result_b.host_version.clone(),
                    key: key.clone(),
                    value_a: va.clone(),
                    value_b: vb.clone(),
                });
            }
        }
    }

    // Compare events, including unmatched trailing events
    let max_events = result_a.events.len().max(result_b.events.len());
    for index in 0..max_events {
        let event_a = result_a.events.get(index);
        let event_b = result_b.events.get(index);

        if event_a != event_b {
            event_differences.push(EventDifference {
                index,
                event_a: event_a.cloned(),
                event_b: event_b.cloned(),
            });

            if let (Some(ea), Some(eb)) = (event_a, event_b) {
                divergences.push(DeterminismError::EventDivergence {
                    host_version_a: result_a.host_version.clone(),
                    host_version_b: result_b.host_version.clone(),
                    index,
                    event_a: ea.clone(),
                    event_b: eb.clone(),
                });
            }
        }
    }

    let is_deterministic = divergences.is_empty()
        && state_differences.is_empty()
        && event_differences.is_empty();

    DeterminismComparison {
        host_version_a: result_a.host_version.clone(),
        host_version_b: result_b.host_version.clone(),
        is_deterministic,
        divergences,
        state_differences,
        event_differences,
    }
}

fn format_optional<T: std::fmt::Display>(value: Option<T>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "<missing>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(
        host_version: &str,
        state: &[(&str, &str)],
    ) -> HostVersionResult {
        let mut state_snapshot = HashMap::new();
        for (k, v) in state {
            state_snapshot.insert((*k).to_string(), (*v).to_string());
        }
        HostVersionResult {
            host_version: host_version.to_string(),
            accepted_count: 1,
            rejected_count: 0,
            state_snapshot,
            events: vec!["event".to_string()],
            error_code: None,
            metadata: ExecutionMetadata {
                gas_consumed: Some(100),
                cpu_instructions: Some(10),
                memory_bytes: Some(20),
                execution_time_ms: Some(1),
                peak_memory_bytes: Some(30),
                custom: HashMap::new(),
            },
        }
    }

    #[test]
    fn identical_comparisons_have_stable_ordering() {
        // Same logical state, different insertion orders.
        let a = make_result(
            "v1",
            &[("alpha", "1"), ("beta", "2"), ("gamma", "3"), ("delta", "4")],
        );
        let b = make_result(
            "v2",
            &[("delta", "4"), ("gamma", "3"), ("beta", "2"), ("alpha", "1")],
        );

        let report_a = compare_results(&a, &b);
        let report_b = compare_results(&b, &a);

        assert!(report_a.is_deterministic);
        assert!(report_a.state_differences.is_empty());
        assert!(report_a.divergences.is_empty());
        assert_eq!(
            serde_json::to_string(&report_a).unwrap(),
            serde_json::to_string(&report_b).unwrap()
        );
    }

    #[test]
    fn differing_states_report_sorted_differences() {
        let a = make_result(
            "v1",
            &[("gamma", "3"), ("alpha", "1"), ("beta", "2")],
        );
        let b = make_result(
            "v2",
            &[("beta", "20"), ("gamma", "30"), ("alpha", "10")],
        );

        let report = compare_results(&a, &b);

        assert!(!report.is_deterministic);
        let keys: Vec<&str> = report
            .state_differences
            .iter()
            .map(|d| d.key.as_str())
            .collect();
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);

        // Reversing insertion order must not change serialized output.
        let a2 = make_result(
            "v1",
            &[("beta", "2"), ("gamma", "3"), ("alpha", "1")],
        );
        let b2 = make_result(
            "v2",
            &[("alpha", "10"), ("gamma", "30"), ("beta", "20")],
        );
        let report2 = compare_results(&a2, &b2);
        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            serde_json::to_string(&report2).unwrap()
        );
    }
}
