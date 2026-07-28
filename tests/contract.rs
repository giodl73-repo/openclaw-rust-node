use openclaw_node::{
    ContractStatus, FixtureError, load_manifest, load_pin, validate_fixture, verify_tarball_sha512,
};
use std::{fs, path::Path};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn protocol_pin_is_immutable_and_explicitly_prerelease() {
    let manifest = load_manifest(&Path::new(ROOT).join("protocol/node-contract.json"))
        .expect("manifest should decode");
    let pin = load_pin(
        &Path::new(ROOT)
            .join("protocol")
            .join(&manifest.protocol_pin),
    )
    .expect("manifest protocol pin should decode");
    assert_eq!(pin.package, "@openclaw/gateway-protocol");
    assert_eq!(pin.version, "2026.7.2-beta.5");
    assert_eq!(pin.protocol_version, 4);
    assert_eq!(pin.minimum_node_protocol_version, 3);
    assert!(
        !pin.release_ready,
        "a beta-only pin must not be release-ready"
    );
}

#[test]
fn manifest_is_well_formed_and_surfaces_known_upstream_gaps() {
    let manifest = load_manifest(&Path::new(ROOT).join("protocol/node-contract.json"))
        .expect("manifest should decode");
    manifest
        .validate()
        .expect("manifest invariants should hold");

    let missing: Vec<&str> = manifest
        .contracts
        .iter()
        .filter(|entry| entry.status == ContractStatus::MissingUpstream)
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(
        missing,
        [
            "connect.challenge",
            "node.pair.requested",
            "node.pair.resolved",
            "node.invoke.cancel",
            "disconnect-cleanup"
        ]
    );
}

#[test]
fn tarball_digest_verifier_accepts_only_the_expected_bytes() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let tarball = directory.path().join("package.tgz");
    fs::write(&tarball, b"fixture").expect("fixture tarball should be written");
    let expected = "94e950eda4a870e6565d44e14b90f16c0a381cd69304529c81f6bd408af13c515516b8417e005fc31093faa4b13de92436456dd25289ab2867c3a576fa948449";
    verify_tarball_sha512(&tarball, expected).expect("known digest should match");
    assert!(verify_tarball_sha512(&tarball, &"0".repeat(128)).is_err());
}

#[test]
fn accepted_fixtures_match_the_pinned_projection() {
    for (contract, file) in [
        ("device.pair.requested", "device-pair-requested.json"),
        ("node.invoke.request", "node-invoke-request.json"),
        ("node.invoke.input", "node-invoke-input.json"),
        ("node.invoke.result", "node-invoke-result.json"),
    ] {
        let json = fs::read_to_string(Path::new(ROOT).join("fixtures/accepted").join(file))
            .expect("fixture should be readable");
        validate_fixture(contract, &json).unwrap_or_else(|error| {
            panic!("{file} should satisfy {contract}: {error:?}");
        });
    }
}

#[test]
fn malformed_published_fixture_is_rejected() {
    let json = fs::read_to_string(
        Path::new(ROOT).join("fixtures/rejected/node-invoke-request-missing-command.json"),
    )
    .expect("fixture should be readable");
    assert!(matches!(
        validate_fixture("node.invoke.request", &json),
        Err(FixtureError::InvalidPayload(_))
    ));
}

#[test]
fn schema_string_bounds_are_enforced() {
    let empty_command = r#"{"type":"event","event":"node.invoke.request","payload":{"id":"invoke-1","nodeId":"node-1","command":""}}"#;
    assert!(matches!(
        validate_fixture("node.invoke.request", empty_command),
        Err(FixtureError::InvalidPayload(_))
    ));

    let oversized_input = serde_json::json!({
        "type": "event",
        "event": "node.invoke.input",
        "payload": {
            "id": "invoke-1",
            "nodeId": "node-1",
            "seq": 0,
            "payloadJSON": "x".repeat(16_385)
        }
    });
    assert!(matches!(
        validate_fixture("node.invoke.input", &oversized_input.to_string()),
        Err(FixtureError::InvalidPayload(_))
    ));
}

#[test]
fn optional_schema_properties_reject_explicit_null() {
    let null_timeout = r#"{"type":"event","event":"node.invoke.request","payload":{"id":"invoke-1","nodeId":"node-1","command":"example.status","timeoutMs":null}}"#;
    assert!(matches!(
        validate_fixture("node.invoke.request", null_timeout),
        Err(FixtureError::InvalidPayload(_))
    ));
}

#[test]
fn unpublished_contract_cannot_accidentally_be_claimed() {
    for contract in [
        "connect.challenge",
        "node.pair.requested",
        "node.pair.resolved",
        "node.invoke.cancel",
        "disconnect-cleanup",
        "node.pending",
    ] {
        let json = format!(r#"{{"type":"event","event":"{contract}","payload":{{}}}}"#);
        assert_eq!(
            validate_fixture(contract, &json),
            Err(FixtureError::UnsupportedContract(contract.into()))
        );
    }
}
