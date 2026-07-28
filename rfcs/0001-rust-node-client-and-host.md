# RFC 0001: Rust Node Client and Headless Node Host

- Status: Draft
- Authors: Gio Della-Libera
- Created: 2026-07-28
- Target: OpenClaw node ecosystem
- Tracking implementation: this repository

## Summary

Build a reusable Rust implementation of the existing OpenClaw node-client
contract and a small headless node-host binary.

The project consumes OpenClaw's published Gateway protocol and preserves
OpenClaw as the sole owner of wire schemas, pairing semantics, command
semantics, and execution policy.

The initial implementation delivers transport, pairing, invocation, a custom
command runtime, and low-risk headless commands. `system.run` is a separately
gated final phase that requires shared execution-policy conformance with the
TypeScript reference host.

## Motivation

OpenClaw currently has:

- a mature TypeScript headless node host in `src/node-host`;
- a published machine-readable Gateway contract in
  `@openclaw/gateway-protocol`;
- native and standalone node implementations across several platforms.

Rust applications currently must either supervise a JavaScript runtime,
independently reconstruct the protocol, or depend on an implementation without
an upstream compatibility contract.

A reusable Rust client would support native desktop applications, appliances,
embedded systems, headless servers, and other products that need to participate
as normal paired OpenClaw nodes.

## Goals

1. Provide an embeddable asynchronous Rust node client.
2. Implement normal OpenClaw device identity, authentication, and pairing.
3. Support the documented current and N-1 node protocol window.
4. Handle invocation request, input, progress, cancellation, and result flows.
5. Provide a bounded command-handler runtime.
6. Ship a small cross-platform headless node binary.
7. Prove compatibility through black-box tests against real OpenClaw Gateways.
8. Match the TypeScript reference security policy before exposing
   `system.run`.
9. Preserve Gateway removal and credential-revocation behavior so a revoked
   node disconnects, refuses new work, and fails in-flight invocations.

## Non-goals

- Defining or forking the OpenClaw Gateway protocol.
- Replacing the TypeScript node host.
- Changing node pairing, roles, permissions, or `node.invoke` wire names.
- Loading TypeScript node-host plugins in Rust.
- Reimplementing every platform capability.
- Adding product-specific Gateway fields or commands.
- Treating hosting infrastructure or credential brokers as paired nodes.

## Ownership

OpenClaw remains authoritative for:

- wire protocol versions and schemas;
- client identity and handshake payloads;
- device pairing and token behavior;
- node command and capability semantics;
- effective command-surface narrowing;
- invocation lifecycle and error semantics;
- execution approvals and security policy.

This project owns:

- Rust wire representations checked against the published schema;
- Rust WebSocket, TLS, authentication, and reconnect implementation;
- Rust command-handler ergonomics and bounded runtime behavior;
- Rust packaging, release artifacts, and conformance automation.

If an implementation need requires a wire or semantic change, that change must
land in OpenClaw first.

## Design

### Protocol source

Each release pins:

- an OpenClaw release;
- its `@openclaw/gateway-protocol` package version;
- the wire protocol version;
- the minimum accepted node protocol version;
- the integrity hash of the generated `protocol.schema.json`.

The schema is vendored for offline builds. CI compares it with the corresponding
published artifact and checks Rust types and fixtures against it.

Release-gating CI uses immutable OpenClaw release tags and artifacts. A separate
non-gating canary may run against OpenClaw `main` to detect upcoming drift, but a
moving branch cannot define release reproducibility.

### Connection lifecycle

The client:

1. opens a bounded WebSocket or WSS connection;
2. validates system-root trust or an explicit leaf-certificate fingerprint;
3. loads or creates an Ed25519 device identity;
4. signs the Gateway challenge with the canonical device-auth payload;
5. connects with `role: "node"` and `client.mode: "node"`;
6. handles pairing-required, credential-required, incompatible-version, and
   terminal reconnect-pause states;
7. persists a Gateway-bound device token after approval;
8. advertises its declared command, capability, and permission ceilings;
9. publishes optional dynamic tools or skills only when supported.

Protocol incompatibility or revoked credentials stop automatic reconnect and
surface an actionable not-ready state.

Gateway-initiated node removal or credential revocation is authoritative. A
revoked connection is closed, in-flight invocations fail, new work is refused,
and the client does not silently create a new identity or auto-pair.

### Credential storage

Credential storage is supplied through a trait.

Preferred implementations use:

- OS keyrings;
- platform keystores;
- TPM-backed storage;
- an embedding application's secret store.

A file implementation is permitted only with:

- owner-only permissions;
- symlink and path hardening;
- bounded reads;
- atomic replacement;
- Gateway binding;
- temporary-buffer zeroization;
- explicit rotation, revocation, and deletion.

Node credentials must never be reused for operator or hosted-service roles.

### Invocation lifecycle

The client accepts:

- `node.invoke.request`;
- ordered invocation input;
- invocation cancellation.

Handlers receive:

- command name;
- validated or raw JSON parameters;
- invocation ID and optional idempotency key;
- deadline and cancellation signal;
- a bounded ordered progress writer.

Handlers return a structured success or error result. The runtime enforces
bounds on frames, queued work, concurrent invocations, progress, and final
output.

Disconnect fails in-flight invocations. The client does not replay them
automatically. Commands that support caller retries own durable idempotency
handling.

OpenClaw also defines bounded pending node work through drain and acknowledgement
methods. V1 must either implement that queue or explicitly omit every capability
that relies on it. The conformance matrix records which pending-work classes the
client supports.

### Activation and readiness

An embedding application controls when commands become active.

The node is ready only when it is:

- connected;
- authenticated;
- paired;
- protocol-compatible;
- activated by the embedding application;
- below critical queue and concurrency limits.

Credential revocation, protocol incompatibility, terminal reconnect pause, or
critical saturation produce a not-ready state.

### Handler isolation

Library-owned tasks and buffers are bounded. A handler panic is converted to a
structured failure when the build uses unwinding and cannot unwind into the
embedding application.

Applications that require stronger fault isolation may continue to run the
headless binary as a separate process.

### Execution security

Transport compatibility is not sufficient for `system.run`.

Before the Rust binary advertises execution commands, both the Rust and
TypeScript hosts must pass one OpenClaw-owned, language-neutral corpus covering:

- prepared-plan canonicalization;
- command allowlists;
- dangerous-command classification;
- approval binding;
- argv and shell edge cases;
- environment filtering;
- working-directory validation;
- executable and file identity binding;
- timeout and cancellation;
- output bounds and progress;
- audit event behavior;
- malformed and unrepresentable requests.

The Rust host must not introduce a weaker fallback or claim partial
`system.run` compatibility.

If no upstream owner and corpus are available, the project stops at the useful
read-only headless host delivered by PR 4. It does not independently invent
execution policy or advertise partial `system.run` support.

Audit, approval, revocation, and emergency-disable semantics remain OpenClaw
contracts. Rust must match the reference host and may not add a weaker fallback
or claim stronger guarantees that the Gateway does not provide.

## Proposed public API

The exact names remain provisional.

```rust
let node = NodeClient::builder()
    .gateway(gateway)
    .identity_store(identity_store)
    .command("system.which", which_handler)
    .command("example.status", status_handler)
    .build()
    .await?;

node.activate().await?;
node.run().await
```

Handlers should be implementable without depending on internal transport types.

## Compatibility

- The supported matrix is keyed by both OpenClaw release and wire protocol
  version.
- Node-version coverage exercises the client range declared by the pinned
  `MIN_NODE_PROTOCOL_VERSION` and `PROTOCOL_VERSION`; testing predecessor
  Gateway releases is a separate release-compatibility dimension.
- Current and supported predecessor Gateways run in CI.
- Unknown documented additive fields are ignored safely.
- Unsupported optional methods degrade through explicit discovery or documented
  `INVALID_REQUEST` handling.
- Incompatible protocol versions fail visibly and do not retry forever.

## Observability

The client exposes structured:

- connection state;
- pairing and credential state;
- negotiated protocol version;
- declared and effective command surfaces;
- readiness reason;
- reconnect attempts and terminal pause reason;
- invocation counts, latency, cancellation, timeout, overload, and failures;
- bounded redacted diagnostics.

Secrets, signatures, tokens, raw approval material, and unrestricted command
output are never included in diagnostics.

The headless binary provides machine-readable structured logs and a local
health/readiness surface. A metrics exporter may be added without making it a
condition of the embeddable client API.

## Release posture

- MIT license.
- Documented minimum supported Rust version.
- No crate or binary is published before PR 4.
- Public crate and binary names remain provisional until OpenClaw maintainers
  accept repository and release ownership.
- Published crates use crates.io Trusted Publishing or an equivalent
  short-lived release identity.
- Every published binary is signed; unavailable signing infrastructure blocks
  that platform artifact rather than producing an unsigned release.
- Checksums, SBOM, dependency audit, and build provenance.
- Vendored protocol pins and offline conformance fixtures.
- Compatibility table mapping crate version, OpenClaw release range, wire
  protocol, and minimum supported Rust version.
- Explicit support and end-of-life table, including N-1 duration and MSRV
  change policy.
- Fresh-environment install and artifact smoke tests before tagging.
- Release runbook covering partial publish failure, crate yanking, artifact
  withdrawal, advisory publication, and restoration.
- No "official" label until OpenClaw maintainers accept repository and release
  ownership.

The first release should remain one crate plus one binary. A later multi-crate
split requires coordinated publish ordering and a tested partial-failure/yank
runbook because crates.io publication is not atomic.

## Pull request plan

Six PRs are proposed. The sequence is intentionally linear so each change can
be reviewed and proven independently.

### PR 0: RFC and conformance contract

Scope:

- this RFC;
- terminology and ownership;
- pinned protocol artifact design;
- test matrix and acceptance criteria;
- no runtime implementation.

Exit gate:

- OpenClaw maintainers agree on repository placement, protocol authority,
  compatibility window, V1 command boundary, naming, and release ownership;
- the project records one of two explicit outcomes:
  - accepted path: proceed toward an official OpenClaw home and support model;
  - independent path: remain clearly experimental and unofficial, publish no
    official-looking crate or binary names, and make no upstream compatibility
    guarantee.

### PR 1: Protocol pin and conformance harness

Scope:

- one experimental crate;
- vendored `protocol.schema.json` plus integrity metadata;
- minimal frame types and strict fixture decoding;
- Gateway test harness;
- accepted and rejected handshake/invocation fixtures.

Exit gate:

- fixtures pass against pinned current and supported predecessor release
  artifacts;
- a non-gating OpenClaw `main` canary reports upcoming drift separately;
- schema drift fails CI.

### PR 2: Transport, identity, authentication, and pairing

Scope:

- WebSocket/WSS and TLS fingerprint pinning;
- Ed25519 identity and signed challenge;
- credential-store trait and hardened file implementation;
- device-token persistence and rotation;
- reconnect and terminal-pause behavior;
- node handshake and pairing.

Exit gate:

- real Gateway pairing succeeds;
- invalid signature, stale nonce, token mismatch, revocation, TLS mismatch, and
  incompatible protocol all fail deterministically.
- revocation during an invocation closes the node session and fails the
  invocation without automatic re-pairing.

### PR 3: Invocation runtime and read-only proof

Scope:

- command registration;
- activation and readiness;
- request/result, input, progress, and cancellation;
- supported pending-work drain and acknowledgement, or an explicit capability
  exclusion proving no advertised command depends on pending work;
- queue, concurrency, payload, progress, and output bounds;
- structured overload and handler-failure behavior;
- one harmless read-only example command.

Exit gate:

- accepted invocation and structured rejection;
- timeout, cancellation, disconnect, malformed input, saturation, and panic
  containment pass end to end.

### PR 4: Headless binary and low-risk commands

Scope:

- `openclaw-node` binary;
- configuration and diagnostics;
- `system.which` and runtime status;
- systemd, launchd, Windows Service, and foreground examples;
- initial cross-platform artifacts.

Exit gate:

- behavior matches the TypeScript reference for the promoted commands;
- release artifact smoke tests and compatibility matrix pass;
- platform-specific credential permissions, structured logs,
  health/readiness, and graceful shutdown are proven.

At this point the project is useful without remote shell execution.

### PR 5: Execution-policy parity

Prerequisite:

- a named OpenClaw owner publishes or accepts the shared execution-policy
  corpus;
- an OpenClaw-owned emergency disable or revocation path can stop affected
  versions from advertising or executing `system.run`.

Scope:

- `system.run.prepare`;
- approvals and allowlists;
- `system.run`;
- environment, cwd, argv, executable/file, timeout, cancellation, progress,
  output, and audit parity;
- shared adversarial fixtures.

Exit gate:

- both TypeScript and Rust hosts pass the same corpus;
- no Rust-specific policy exception or fallback exists;
- a named OpenClaw security owner accepts the implementation and release
  rollback runbook.

## Alternatives considered

### Keep using the TypeScript host

This remains valid and is the baseline for performance, compatibility, and
security comparisons. The Rust project should stop if it cannot justify its
maintenance and release cost.

### Add Rust directly under `src/node-host`

Rejected initially because it couples Cargo and native release concerns to the
npm package while providing little source reuse.

### Keep only an application-specific implementation

Rejected because it creates a downstream protocol fork and does not benefit the
OpenClaw ecosystem.

### Implement `system.run` immediately

Rejected because execution-policy drift is a higher risk than transport
implementation. A useful read-only node should ship first.

## Risks

- Security-policy drift between TypeScript and Rust.
- Protocol changes without a language-neutral compatibility artifact.
- A second implementation exceeding available maintainer capacity.
- Platform credential-store and service-packaging complexity.
- False confidence from wire compatibility without behavioral parity.
- Premature crate/API stabilization.

## Evidence that would reverse this decision

Stop or narrow the project if:

- OpenClaw cannot provide a stable language-neutral node contract;
- execution-policy parity cannot be maintained economically;
- the existing TypeScript host has lower total cold-start, memory, reliability,
  and packaging cost for intended adopters;
- no upstream maintainer accepts long-term compatibility ownership;
- the useful command set requires product-specific wire exceptions.

## Open questions

1. Should the eventual official home be a standalone OpenClaw repository or the
   main monorepo?
2. Which OpenClaw maintainers own node protocol and Rust release decisions?
3. Which read-only command should be the first conformance proof?
4. Should protocol Rust types be generated, handwritten and schema-checked, or
   use a hybrid approach?
5. Where should the shared execution-policy corpus live?
6. Should OpenClaw's Linux companion eventually consume these crates?
7. Which existing OpenClaw revocation or command-disable mechanism is the
   canonical emergency stop for a faulty Rust execution release?
