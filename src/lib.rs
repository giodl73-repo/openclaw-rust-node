//! A reusable, asynchronous `OpenClaw` node client.
//!
//! The crate owns transport and request/event ergonomics. `OpenClaw` remains
//! authoritative for the Gateway protocol, pairing, and command semantics.

mod client;
mod identity;
mod manifest;
mod wire;

pub use client::{
    ClientError, ConnectAuth, DeviceProof, Event, NodeClient, NodeClientConfig, NodeConnectOptions,
    NodeSession,
};
pub use identity::{IdentityError, NodeIdentity};
pub use manifest::{
    ContractEntry, ContractStatus, NodeContractManifest, ProtocolPin, load_manifest, load_pin,
};
pub use wire::{FixtureError, validate_fixture};

use sha2::{Digest, Sha512};
use std::{fmt::Write as _, fs, path::Path};

/// Verify the SHA-512 digest of a downloaded npm tarball against the immutable
/// hexadecimal digest recorded in the protocol pin.
/// # Errors
///
/// Returns an I/O error when the tarball cannot be read, or the expected and
/// actual digest when integrity verification fails.
pub fn verify_tarball_sha512(path: &Path, expected_hex: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let digest = Sha512::digest(bytes);
    let mut actual = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut actual, "{byte:02x}").expect("writing to a String cannot fail");
    }
    if actual == expected_hex {
        Ok(())
    } else {
        Err(format!(
            "SHA-512 mismatch: expected {expected_hex}, got {actual}"
        ))
    }
}
