# OpenClaw crate snapshot

This repository retains a buildable snapshot of the two Rust crates proposed
to the OpenClaw monorepo so Microsoft adopters can take reviewed drops and give
feedback before the complete upstream series lands.

## Provenance

- Source repository: `openclaw/openclaw`
- Source fork: `giodl73-repo/openclaw`
- Cumulative source branch: `agent/rust-sidecar-runtime-bridge`
- Source commit: `71c1c8cb23c5647dc07fd4ee1f8663068c92a482`
- Upstream review: [OpenClaw PR #116863](https://github.com/openclaw/openclaw/pull/116863)
- Logical prerequisites: [#116050](https://github.com/openclaw/openclaw/pull/116050)
  and [#116450](https://github.com/openclaw/openclaw/pull/116450)

The files under `crates/` and these six files under `test/fixtures/` are copied
without product-specific changes:

- `node-invoke-lifecycle-contract.json`
- `node-runtime-integration-contract.json`
- `node-sidecar-protocol-v1.json`
- `node-sidecar-negotiation-v1.json`
- `node-sidecar-handshake-v1.json`
- `node-sidecar-runtime-v1.json`

## Boundary

This is an integration snapshot, not an independent protocol fork. OpenClaw
remains authoritative for the crates and fixtures. Changes intended for the
shared implementation should be proposed upstream first, then synchronized
here from an exact reviewed commit.

The snapshot is the single shared runtime consumed by product integrations;
adopters do not maintain downstream copies with product-specific patches.
Microsoft-owned integration belongs under `integrations/edge-chromium/`, and
black-box composition evidence belongs under `tests/cross-repo/`. See
[`REPOSITORY_LAYOUT.md`](REPOSITORY_LAYOUT.md) for the complete synchronization
contract.

The snapshot includes authenticated framing, negotiation, immutable runtime
configuration, ordinary-command bridging, lifecycle, admission, cancellation,
and conformance fixtures. Its test harness launches a separate OS child and
completes configuration, exact manifest acknowledgement, admission, invocation,
and result exchange over loopback TCP. This proves the generic process and
transport boundary; it does not provide production process supervision,
protected credential bootstrap, product-selected IPC, artifact verification,
product audit integration, runtime selection, rollout, or rollback.

## Validation

Run the snapshot with its OpenClaw Rust 1.93 MSRV:

```console
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --workspace --locked
cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path crates/Cargo.toml --workspace --no-deps
```

The repository CI runs these independently from the historical root prototype.
At the synchronized source head, the workspace contains 124 passing Rust tests.
