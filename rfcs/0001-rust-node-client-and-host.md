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

The initial evidence slice delivers transport, separate device and node
capability approval flows, and one bounded custom command. Invocation streaming,
the generic headless binary, packaging, and built-in read-only commands follow
as separate review units. `system.run` is a separately gated post-V1 program
that requires shared execution-policy conformance with the TypeScript reference
host.

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
2. Implement OpenClaw device identity and authentication, device pairing, and
   the separate node capability-approval flow.
3. Support the documented current and N-1 node protocol window.
4. Handle invocation request and result first, then add input, progress, and
   cancellation behind their own conformance gate.
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
- Publishing dynamic plugin tools or skills in V1.
- Supporting durable pending work in V1 unless a concrete first adopter needs
  it and its capability is declared explicitly.

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

The full schema is an integrity pin, not the Rust implementation scope. The
repository also maintains a node-contract manifest that enumerates every
required node-facing contract: connect and `hello-ok`, structured connect
errors, device pairing, node capability approval, pending work, invocation
request/input/cancellation/progress/result, and disconnect/revocation behavior.
Only changes that intersect this manifest block the node compatibility gate;
unrelated Gateway schema changes remain visible in the drift report without
forcing Rust type churn.

Every manifest entry must resolve to a published schema, documented error
contract, or conformance fixture owned by OpenClaw. The current TypeScript host
locally coerces `node.invoke.cancel` without a corresponding published payload
schema. That and any similar gaps must be fixed and released in OpenClaw before
the Rust project implements or claims conformance for the affected behavior.
The Rust repository must not fill contract gaps by inventing a wire shape.

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
6. handles bootstrap-credential, device-pairing-required,
   credential-required, incompatible-version, and terminal reconnect-pause
   states;
7. persists a Gateway-bound device token after device approval;
8. advertises its declared command, capability, and permission ceilings and
   separately tracks node capability approval;
9. keeps dynamic tools, skills, and pending-work capabilities absent from V1
   advertisement unless a later, separately reviewed module implements them.

Protocol incompatibility or revoked credentials stop automatic reconnect and
surface an actionable not-ready state.

Gateway-initiated node removal or credential revocation is authoritative. A
revoked connection is closed, in-flight invocations fail, new work is refused,
and the client does not silently create a new identity or auto-pair.

### Two-layer pairing and enrollment

OpenClaw pairing has two distinct approval layers:

1. **Device pairing** authenticates the signed Ed25519 identity for `role:
   "node"`, gates the connection, and issues the Gateway-bound device token.
2. **Node capability approval** compares the commands, capabilities, and
   permissions declared on connect with the approved surface. A new or widened
   surface creates a separate `node.pair.requested` request and remains
   ineffective until it is approved or rejected through `node.pair.*`.

The client exposes these as separate states with separate request identifiers,
retry guidance, expiry, and diagnostics. A successful device pairing does not
mean commands are invocable. Reconnect after device approval and capability
reapproval are explicit conformance cases.

The supported V1 enrollment baseline is manual device and node-capability
approval. OpenClaw's default SSH auto-approval currently probes
`openclaw node identity --json`; a standalone `openclaw-node` binary does not
claim that path unless it installs a compatible identity-inspection shim or
OpenClaw first accepts a configurable probe contract. Failure to satisfy the
SSH probe falls back to the documented manual flow rather than weakening
identity verification.

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

OpenClaw defines both connected-node pending consumption (`node.pending.pull`
and `node.pending.ack`) and durable offline work (`node.pending.enqueue` and
`node.pending.drain`). V1 advertises no capability that relies on either queue.
A later adopter-driven module must identify the exact work class, implement its
queue semantics, and add conformance coverage in a dedicated PR.

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
generic headless host and optional `system.which` parity delivered by R7-R9. It
does not independently invent execution policy or advertise partial
`system.run` support.

Audit, approval, revocation, and emergency-disable semantics remain OpenClaw
contracts. Rust must match the reference host and may not add a weaker fallback
or claim stronger guarantees that the Gateway does not provide.

## Proposed public API

The exact names remain provisional.

```rust
let node = NodeClient::builder()
    .gateway(gateway)
    .identity_store(identity_store)
    .command("example.status", status_handler)
    .build()
    .await?;

node.activate().await?;
node.run().await
```

Handlers should be implementable without depending on internal transport types.
OpenClaw-owned command namespaces, including `system.*`, are reserved. Custom
handler registration cannot replace a built-in command implementation. Built-in
commands are exposed only by explicit runtime modules that implement their
OpenClaw-owned validation, authorization, and result semantics.

## Compatibility

- The supported matrix is keyed by both OpenClaw release and wire protocol
  version.
- Node-version coverage exercises the client range declared by the pinned
  `MIN_NODE_PROTOCOL_VERSION` and `PROTOCOL_VERSION`; testing predecessor
  Gateway releases is a separate release-compatibility dimension.
- The handshake advertises a supported range; it is not assumed to negotiate a
  lower wire dialect. The client records its advertised range, the
  server-reported protocol, and whether the Gateway admitted it through the
  legacy node compatibility window.
- Current and supported predecessor Gateways run in CI.
- Unknown documented additive fields are ignored safely.
- Unsupported optional methods degrade through explicit discovery or documented
  `INVALID_REQUEST` handling.
- Incompatible protocol versions fail visibly and do not retry forever.

## Observability

The client exposes structured:

- connection state;
- device-pairing, node-capability-approval, and credential state;
- advertised protocol range, server-reported protocol, and legacy-window
  classification;
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
- No crate or binary is published before the packaging and experimental-release
  gate.
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

This is a cross-repository sequence. Merge each proved boundary as it becomes
ready; do not hold the entire implementation in one long stack.

### OpenClaw U1: Complete the published node event contract

Scope:

- publish the `node.invoke.cancel` payload schema and type;
- add language-neutral fixtures for request, result, input, progress,
  cancellation, structured errors, and disconnect cleanup;
- publish a supported node-contract projection or an equally durable mapping.

Exit gate:

- the contract ships in an immutable OpenClaw release artifact;
- Rust does not implement wire cancellation or claim its conformance before
  that release.

### Rust R0: RFC, ownership, and evidence gates

Scope:

- this RFC, terminology, ownership, source-backed contract inventory, explicit
  V1 exclusions, two-layer pairing model, compatibility-window terminology,
  and official-versus-independent outcome;
- no runtime implementation.

Exit gate:

- every required behavior has an OpenClaw-owned schema, documented contract,
  accepted fixture, upstream work item, or explicit removal from V1;
- repository, naming, support, and release ownership are accepted or the
  project remains visibly experimental and unofficial.

### Rust R1: Protocol pin and conformance harness

Scope:

- one experimental crate;
- immutable OpenClaw release and schema integrity pins;
- narrow node-contract manifest, minimal wire types, strict fixture decoding,
  and a real pinned-Gateway harness;
- non-gating OpenClaw `main` drift canary.

Exit gate:

- node-contract drift blocks CI while unrelated schema drift is reported;
- accepted and rejected fixtures are reproducible from released artifacts.

### Rust R2: WebSocket transport state machine

Scope:

- challenge ordering, frame parsing, request correlation, timeouts, sequence
  gaps, reconnect backoff, close classification, WSS, and leaf fingerprint
  pinning;
- host-owned authentication and identity hooks only.

Exit gate:

- scripted-server and pinned-Gateway tests cover connect success, malformed
  frames, timeout cleanup, disconnect, protocol failure, and reconnect policy.

### Rust R3: Identity, authentication, and device pairing

Scope:

- Ed25519 identity, canonical challenge signing, credential-store trait,
  hardened file implementation, device tokens, revocation, terminal pause, and
  manual enrollment diagnostics;
- document the unsupported default SSH probe unless a compatible shim or
  upstream hook lands separately.

Exit gate:

- device approval, reconnect, persistence, invalid signature, stale nonce,
  token rejection, TLS mismatch, revocation, and explicit re-pairing pass end to
  end.

### Rust R4: Capability approval, activation, and minimal command

Scope:

- declared versus effective commands, node capability request and reapproval,
  embedding activation, readiness, and one fixed bounded custom status command;
- the minimum request/result path needed to execute that fixed proof command,
  without a public handler-registration runtime;
- no pending work, dynamic tools/skills, streaming invocation, or `system.*`.

Exit gate:

- device and node-capability request IDs are proven distinct;
- the command is unavailable before capability approval and works only after
  approval and activation;
- rejection, reconnect, and widened-surface reapproval pass end to end.

This is the smallest evidence slice and the first mandatory continue-or-stop
review.

### Rust R5: Reusable bounded invocation runtime

Scope:

- replace the fixed proof handler with the public command-registration runtime;
- complete request/result lifecycle, deadlines, local cancellation-token
  plumbing, optional node-facing idempotency keys, queue/concurrency/output
  bounds, structured overload, malformed payloads, panic containment, and
  disconnect cleanup;
- no automatic replay.

Exit gate:

- accepted result, structured rejection, timeout, saturation, handler failure,
  and disconnect behavior pass against a real Gateway.

### Rust R6: Streaming invocation lifecycle

Scope:

- ordered input, bounded progress, and wire cancellation after U1 ships;
- late, duplicate, and out-of-order frame handling plus cancellation races.

Exit gate:

- Rust and TypeScript pass the same released lifecycle fixtures and the real
  Gateway tests cover timeout, cancellation, disconnect, and output caps.

### Rust R7: Generic headless binary

Scope:

- configuration, foreground operation, local health/readiness, structured
  redacted diagnostics, graceful shutdown, and the custom status command;
- no OpenClaw-owned `system.*` command.

Exit gate:

- fresh-machine foreground smoke and supported-platform behavior pass without
  relying on release packaging.

### Rust R8: Packaging and experimental release

Scope:

- service examples, signed supported-platform artifacts, checksums, SBOM,
  provenance, dependency audit, install smoke, support/compatibility tables,
  and rollback/yank runbook.

Exit gate:

- ownership and naming are resolved before publication; otherwise artifacts
  remain explicitly experimental and unpublished.

### Rust R9: `system.which` parity

Scope:

- exact TypeScript-reference validation, PATH handling, result shape,
  path-disclosure treatment, command policy, and admin-sensitive approval.

Exit gate:

- shared parity fixtures and real-Gateway authorization tests pass, with no
  Rust-specific fallback.

### Deferred programs

Pending work and dynamic tool/skill publication each require a concrete adopter
and a dedicated contract-plus-implementation PR.

Remote execution starts only when a named OpenClaw owner accepts a
language-neutral execution-policy corpus and emergency-disable contract. Rust
work then splits at minimum into preparation/approval semantics,
execution/streaming semantics, and adversarial cross-implementation proof. No
PR advertises `system.run` until the complete gate passes.

### Stack policy

- merge R0 before implementation;
- merge U1 before R6;
- prefer merge-as-you-go, with at most one short-lived dependent Rust PR stacked
  during review latency;
- never stack across an unresolved ownership, schema, security, or release
  gate;
- rebase each dependent PR onto its merged predecessor and review the final diff
  independently.

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
3. Should protocol Rust types be generated, handwritten and schema-checked, or
   use a hybrid approach?
4. Where should the shared execution-policy corpus live?
5. Should OpenClaw's Linux companion eventually consume these crates?
6. Which existing OpenClaw revocation or command-disable mechanism is the
   canonical emergency stop for a faulty Rust execution release?
