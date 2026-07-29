# OpenClaw Rust Node

Experimental Rust node client and headless node host for the
[OpenClaw](https://github.com/openclaw/openclaw) Gateway protocol.

> [!IMPORTANT]
> This is an experimental working client, not an official OpenClaw component.
> Its API and protocol compatibility are not yet stable.

## Goal

Provide a reusable Rust implementation of the existing OpenClaw node contract
without creating a second protocol owner.

The intended result is:

- an embeddable Rust node client;
- a small headless node-host binary;
- black-box conformance tests against real OpenClaw Gateways;
- current and supported N-1 node protocol compatibility;
- execution-policy parity before exposing `system.run`.

OpenClaw remains authoritative for the Gateway schema, pairing model, command
semantics, and reference behavior.

## Proposed architecture

```text
@openclaw/gateway-protocol
          |
          v
openclaw-gateway-protocol  Rust wire types and schema checks
          |
          v
openclaw-node-client       auth, pairing, WebSocket, invoke lifecycle
          |
          v
openclaw-node-runtime      bounded command-handler API
          |
          v
openclaw-node              optional headless binary
```

See [RFC 0001](rfcs/0001-rust-node-client-and-host.md) for the proposed design,
cross-repository PR sequence, security requirements, and evidence gates. The
earlier [proposal](docs/proposal.md) remains as a shorter discussion brief.
The first [role review](rfcs/reviews/0001-craft-role-review-2026-07-28.md)
records accepted findings and current-source verification.

## Principles

- **One protocol owner:** consume OpenClaw's published protocol; do not fork it.
- **Fail visibly:** incompatible protocol or security behavior never silently
  falls back.
- **Bounded runtime:** frames, queues, output, progress, and in-flight work have
  explicit limits.
- **Security parity:** transport compatibility alone is insufficient for
  execution commands.
- **Useful independently:** the library and binary must be generally useful to
  OpenClaw users and integrations.

## First reusable slice

The unpublished `openclaw-node` crate provides a generic asynchronous
`NodeClient`. It enforces node role/mode defaults, waits for the Gateway's
challenge before calling application-owned identity/auth code, correlates
requests, publishes events, bounds request timeouts, and fails pending work on
disconnect. It contains no Lobster or platform-specific behavior.

```rust,no_run
# use openclaw_node::{ConnectAuth, NodeClient, NodeClientConfig, NodeConnectOptions, NodeIdentity};
# use std::convert::Infallible;
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
// Persist these secret bytes in the platform credential store and restore the
// same identity with NodeIdentity::from_secret_bytes on the next launch.
let identity = NodeIdentity::generate()?;
let session = NodeClient::connect(
    NodeClientConfig::new("ws://127.0.0.1:18789"),
    |_nonce| async move {
        Ok::<_, Infallible>(NodeConnectOptions::new("0.1.0", std::env::consts::OS)
                .display_name("My Rust node")
                .auth(ConnectAuth::token("gateway-token"))
                .identity(identity))
    },
).await?;

while let Ok(event) = session.next_event().await {
    println!("{}: {}", event.event, event.payload);
}
# Ok(())
# }
```

`NodeIdentity` generates and signs the canonical Ed25519 v3 Gateway payload,
while callers retain control of secret storage. The successful hello payload
is available through `session.hello()`, including any issued device token for
application-owned persistence.

Command, capability, and permission declarations are withheld until the
embedding explicitly calls `NodeConnectOptions::activate()`. This prevents a
process that is merely able to connect from advertising work it is not ready
to handle. Activation is local readiness, not an approval claim: OpenClaw may
still narrow the surface while a separate node-capability approval is pending,
and the node-role `hello-ok` payload does not disclose that effective surface.
Custom command names must also be admitted by OpenClaw policy, for example by
an installed plugin policy or an operator entry in
`gateway.nodes.allowCommands`; activation never bypasses that allowlist.

Activated sessions expose typed `NodeInvocation` values through
`session.next_invocation()` and complete them with
`session.complete_invocation()`. The embedding owns command routing and handler
logic; the client keeps this transport primitive independent of any one app.

`CommandRuntime` adds the reusable bounded layer: exact-name handlers,
deterministic command advertisement, deadlines, cooperative local
cancellation, strict input/output byte limits, panic containment, and a fixed
concurrency ceiling. It deliberately queues no handler work; saturation
returns `OVERLOADED` immediately. Result delivery is also bounded. If the
Gateway stops acknowledging results and that critical delivery buffer fills,
`run` fails closed with `RuntimeError::DeliverySaturated` so the embedding can
restart the session instead of losing results or growing memory without bound.

```rust,no_run
# use openclaw_node::{CommandRuntime, NodeConnectOptions};
# use serde_json::json;
# fn example() -> Result<(), Box<dyn std::error::Error>> {
let runtime = CommandRuntime::builder()
    .max_concurrency(4)
    .max_output_bytes(64 * 1024)
    .command("example.status", |_context| async {
        Ok(json!({ "ready": true }))
    })
    .build()?;

let connect_options = runtime.activate(NodeConnectOptions::new("0.1.0", "linux"));
// Pass connect_options through NodeClient::connect, then run until disconnect:
// runtime.run(session).await?;
# let _ = connect_options;
# Ok(())
# }
```

Handler cancellation is local in this slice: timeout and disconnect cancel
the token and stop runtime-owned handler tasks. Wire cancellation and streaming
input/progress remain gated on their separately published lifecycle contracts.

`ReconnectPolicy` converts connection failures into explicit reusable actions:

- transient failures use deterministic exponential backoff from one to 30
  seconds and reset after a successful connection;
- ordinary request failures keep the healthy session instead of triggering a
  transport reconnect;
- device-pairing failures preserve the sanitized request ID, reason, requested
  role/scopes, remediation, and Gateway retry hints;
- pairing normally pauses for manual approval, unless the Gateway explicitly
  says to wait and retry;
- credential, protocol, local identity, and endpoint-configuration failures
  pause instead of creating reconnect churn;
- a shared-token mismatch can request exactly one stored-device-token retry,
  but only when the application explicitly marks that token as available for
  an independently trusted endpoint.

The policy neither sleeps nor owns credentials. Applications execute the
returned action, persist identity/device tokens in their own secret store, and
resume after operator approval. Device pairing remains separate from the later
node capability-approval state.

The internal conformance guard pins the first published Gateway protocol beta
and records its actual node-facing coverage in
[`protocol/node-contract.json`](protocol/node-contract.json).

The typed dispatch primitive remains usable directly by embeddings that want
to own runtime policy themselves. The next lifecycle layer is streaming input,
progress, and wire cancellation only after its OpenClaw contracts are released.

## Experimental headless host

The `openclaw-node` binary is a generic foreground wrapper around the same
client and bounded runtime. It is intentionally not a service installer and it
does not register any OpenClaw-owned `system.*` command. Its only handler is a
configurable custom status command.

Start from [`examples/headless-config.json`](examples/headless-config.json).
The JSON file contains no secret values: `authEnv` names the environment
variable holding the Gateway credential, and `identitySecretEnv` names the
variable holding the stable 32-byte Ed25519 secret injected by the operator's
secret store. The identity value may contain unpadded base64url. This R7 host
does not claim to provide a hardened platform credential-file implementation;
platform credential persistence and permissions remain R8 work.

```console
$ export OPENCLAW_NODE_TOKEN='<gateway credential>'
$ export OPENCLAW_NODE_IDENTITY='<32-byte secret as unpadded base64url>'
$ cargo run --bin openclaw-node -- --config examples/headless-config.json
```

The process stays in the foreground, emits one redacted JSON diagnostic per
line on stderr, retries transient transport failures with bounded backoff, and
exits on terminal authentication, pairing, protocol, identity, or
configuration pauses. Approve device pairing and the separately pending node
command surface through OpenClaw, then restart after a terminal pairing pause.

The loopback-only HTTP listener exposes:

- `GET /healthz`: process health;
- `GET /readyz`: `200` only while an activated Gateway session is serving the
  bounded command runtime, otherwise `503`.

`Ctrl-C` first withdraws readiness, closes the Gateway session, cancels owned
handler work, and gives runtime cleanup a bounded shutdown window. On Unix,
`SIGTERM` follows the same path. Service
files, artifact signing, installation, and publication remain R8 work.

## Package acceptance

The `Package acceptance` workflow builds the release binary natively on Linux
x64, Windows x64, and macOS ARM64. Each job stages the binary with the license
and README, creates a platform archive and SHA-256 checksum, extracts that
archive into a clean directory, and runs the packaged binary's `--version` and
`--help` paths before retaining the files as short-lived workflow artifacts.

These CI artifacts are package-acceptance evidence, not releases. They are not
published, signed, attested, supported, or suitable for redistribution. SBOM,
provenance, service installation, rollback, and release ownership remain
separate R8 gates.

The current immutable pin is deliberately marked `releaseReady: false` because
the registry has no stable calendar release of `@openclaw/gateway-protocol` yet
and the pinned beta does not publish every required node event payload.

Run `bash scripts/verify-protocol-pin.sh` to download the pinned npm tarball,
verify its SHA-512 digest, and confirm its version, schema identity, definition
count, and protocol-level constants. Vendoring the complete schema remains an
R1 exit requirement once a stable package artifact is available.

## License

[MIT](LICENSE)
