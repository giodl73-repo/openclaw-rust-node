# Proposal: Rust Node Client and Headless Node Host for OpenClaw

Status: experimental discussion draft  
Date: 2026-07-28

## Summary

Create a supported Rust implementation of the existing OpenClaw node-client
contract and a small headless node-host binary.

The Rust implementation should consume OpenClaw's public Gateway protocol,
remain interoperable with the TypeScript reference node host, and provide a
reusable foundation for native applications, appliances, embedded systems, and
headless Linux or Windows deployments.

This proposal does not change the OpenClaw node protocol and does not make Rust
the source of truth.

## Existing OpenClaw ownership

OpenClaw already has the necessary owner surfaces:

- `packages/gateway-protocol` owns protocol versions, frame schemas, validators,
  and the generated `protocol.schema.json`;
- `src/node-host` owns reference headless-node behavior and execution policy;
- the Gateway owns authentication, pairing, effective capability narrowing,
  invocation routing, deadlines, progress, cancellation, disconnect cleanup,
  and node visibility.

The missing capability is a maintained cross-language client and conformance
path for Rust.

## Repository placement

A standalone repository is preferred over adding Rust to `src/node-host`.

OpenClaw already maintains standalone node and companion repositories,
including:

- [`openclaw/clawgo`](https://github.com/openclaw/clawgo);
- [`openclaw/esp-openclaw-node`](https://github.com/openclaw/esp-openclaw-node);
- [`openclaw/openclaw-windows-node`](https://github.com/openclaw/openclaw-windows-node).

The main OpenClaw repository also contains a Rust/Tauri Linux companion with
Gateway WebSocket, TLS pinning, Ed25519 identity, pairing, and reconnect
behavior that can inform the first implementation.

A standalone repository keeps Cargo, crates.io publishing, cross-compilation,
binary signing, and release ownership separate from the npm release train.

## Target components

The target architecture may eventually contain:

### Protocol crate

- Serde types for Gateway envelopes and node methods.
- Release and wire-protocol version metadata.
- Checks against a pinned, integrity-verified
  `@openclaw/gateway-protocol/protocol.schema.json`.
- Safe handling of documented additive fields.

### Node client crate

- WebSocket and WSS connection management.
- System-root TLS and explicit certificate-fingerprint pinning.
- Ed25519 device identity and signed challenge response.
- Device-token persistence, rotation, and revocation.
- `role: "node"` and `client.mode: "node"` handshake.
- Pairing and credential-required states.
- Current and supported N-1 node protocol behavior.
- Capability, command, permission, plugin-tool, and skill advertisement.
- Invocation request, input, progress, cancellation, and result lifecycle.
- Bounded frames, queues, output, and in-flight work.
- Terminal reconnect-pause behavior for incompatible or revoked credentials.

### Node runtime crate

- Bounded command-handler registration.
- Declared and effective command surfaces.
- Deadline and cancellation propagation.
- Ordered progress output.
- Structured success, overload, cancellation, and failure results.
- Embedding-controlled activation and readiness.
- Idempotency-key exposure without automatic invocation replay.

### Headless binary

- Minimal service configuration and diagnostics.
- `system.which` and low-risk status commands first.
- Cross-platform service examples.
- `system.run.prepare` and `system.run` only after security-policy parity is
  demonstrated.

The first prototype should remain one crate. Package boundaries should be split
only after a working vertical slice proves them.

## Contract authority and drift control

The Rust project must not become a second Gateway protocol owner.

1. OpenClaw's published Gateway protocol remains authoritative.
2. Every Rust release pins an OpenClaw release and wire protocol version.
3. The generated protocol schema is vendored with an integrity hash.
4. Rust types and fixtures are checked against that schema.
5. Black-box tests run against real, pinned Gateway versions.
6. Incompatible behavior blocks release instead of adding a silent fallback.
7. Protocol changes land and release in `openclaw/openclaw` first.

## Credential storage

Key custody must be supplied through a narrow secret-store interface.

- Prefer OS keyrings, platform keystores, TPM-backed storage, or an
  embedding-owned vault.
- Permit a file fallback only with owner-only permissions, bounded reads,
  symlink/path hardening, atomic replacement, and temporary-buffer
  zeroization.
- Bind persisted device tokens to the intended Gateway.
- Support explicit rotation, revocation, and credential deletion.
- Never reuse a node identity as an operator or hosted-service credential.

## Execution security

Wire compatibility is not execution-policy compatibility.

Before advertising `system.run`, the Rust host must match the TypeScript
reference behavior for:

- pairing and approved command-surface narrowing;
- canonical prepared execution plans;
- approval binding to command, argv, working directory, environment, session,
  and relevant executable or file identity;
- command allowlists and dangerous-command classification;
- environment filtering and working-directory validation;
- timeout, progress, cancellation, and disconnect behavior;
- bounded stdout and stderr;
- audit events;
- shell and interpreter edge cases;
- fail-closed behavior when a request cannot be represented safely.

OpenClaw should own a language-neutral execution-policy corpus. Both
implementations must pass the same corpus before the Rust binary claims
`system.run` parity.

## Runtime reliability

- All client-owned queues and buffers have hard bounds.
- Saturation returns a structured overload result.
- Handler failure cannot unwind into an embedding application when unwinding is
  enabled.
- Command advertisement waits for embedding-controlled activation.
- Readiness means connected, authenticated, paired, compatible, activated, and
  able to accept bounded work.
- Protocol incompatibility, credential revocation, terminal reconnect pause,
  and exhausted critical capacity are not-ready states.
- The client does not automatically replay disconnected invocations.
- Commands that support retries own durable idempotency handling.

## V1 plan

### V1.0: transport and embedding

- protocol schema pin;
- device identity and pairing;
- reconnect and protocol negotiation;
- command advertisement;
- request/result, progress, input, and cancellation;
- custom command-handler API;
- Gateway conformance runner;
- one harmless read-only command.

### V1.1: generic headless host

- `system.which`;
- runtime status;
- health and diagnostics;
- service packaging;
- signed cross-platform artifacts.

### V1.2: execution parity

- upstream-owned execution-policy fixtures;
- `system.run.prepare`;
- approvals and allowlists;
- `system.run`;
- shared adversarial security corpus.

## Non-goals

- Replacing the TypeScript node host.
- Changing node pairing or `node.invoke` wire names.
- Loading TypeScript node-host plugins in Rust.
- Reimplementing every platform capability in V1.
- Adding product-specific protocol fields.
- Representing hosting infrastructure or credential brokers as paired nodes.

## Smallest evidence slice

1. Reuse the relevant behavior and tests from OpenClaw's Rust Linux companion
   in one experimental crate.
2. Connect to a real Gateway as `role: "node"`.
3. Complete normal device pairing.
4. Advertise one read-only command.
5. Return one accepted result and one structured rejection.
6. Prove timeout, cancellation, reconnect, overload, malformed input,
   credential revocation, and protocol mismatch behavior.
7. Run against current OpenClaw main and one released supported predecessor.
8. Measure binary size, idle and loaded RSS, connect time, reconnect time, and
   invocation overhead against `openclaw node run`.

## Decisions needed from OpenClaw maintainers

1. Official repository placement and naming.
2. Named maintainers and release ownership.
3. Supported command subset for the initial binary.
4. Protocol and execution-policy conformance artifact publication.
5. Support and end-of-life policy.
6. Whether the Linux companion should later consume the shared crates.
