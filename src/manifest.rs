use serde::Deserialize;
use std::{collections::HashSet, fs, path::Path};

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtocolPin {
    pub package: String,
    pub version: String,
    pub dist_tag: String,
    pub tarball: String,
    pub npm_integrity: String,
    pub sha512_hex: String,
    pub source_release: String,
    pub source_commit: String,
    pub schema_id: String,
    pub schema_definition_count: u32,
    pub protocol_version: u32,
    pub minimum_node_protocol_version: u32,
    pub release_ready: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ContractStatus {
    Published,
    MissingUpstream,
    ExcludedV1,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractEntry {
    pub id: String,
    pub direction: String,
    pub schema_definition: Option<String>,
    pub status: ContractStatus,
    pub upstream: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeContractManifest {
    pub format_version: u32,
    pub protocol_pin: String,
    pub contracts: Vec<ContractEntry>,
}

impl NodeContractManifest {
    /// Check invariants that make the projection useful as a review gate.
    /// # Errors
    ///
    /// Returns a description when IDs are duplicated or a published entry has
    /// no schema definition.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for entry in &self.contracts {
            if !ids.insert(entry.id.as_str()) {
                return Err(format!("duplicate contract id: {}", entry.id));
            }
            if entry.status == ContractStatus::Published && entry.schema_definition.is_none() {
                return Err(format!(
                    "published contract has no schema definition: {}",
                    entry.id
                ));
            }
            if entry.status == ContractStatus::MissingUpstream && entry.upstream.is_none() {
                return Err(format!(
                    "missing upstream contract has no tracking item: {}",
                    entry.id
                ));
            }
        }
        Ok(())
    }
}

/// # Errors
///
/// Returns a descriptive error when the JSON cannot be read or decoded.
pub fn load_pin(path: &Path) -> Result<ProtocolPin, String> {
    let json = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&json).map_err(|error| error.to_string())
}

/// # Errors
///
/// Returns a descriptive error when the JSON cannot be read or decoded.
pub fn load_manifest(path: &Path) -> Result<NodeContractManifest, String> {
    let json = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&json).map_err(|error| error.to_string())
}
