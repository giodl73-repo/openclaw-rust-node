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
# use openclaw_node::{ConnectAuth, NodeClient, NodeClientConfig, NodeConnectOptions};
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let session = NodeClient::connect(
    NodeClientConfig::new("ws://127.0.0.1:18789"),
    |_nonce| async {
        Ok::<_, std::io::Error>(
            NodeConnectOptions::new("0.1.0", std::env::consts::OS)
                .display_name("My Rust node")
                .auth(ConnectAuth::token("gateway-token")),
        )
    },
).await?;

while let Ok(event) = session.next_event().await {
    println!("{}: {}", event.event, event.payload);
}
# Ok(())
# }
```

The internal conformance guard pins the first published Gateway protocol beta
and records its actual node-facing coverage in
[`protocol/node-contract.json`](protocol/node-contract.json).

Next comes reusable identity persistence and pairing support, followed by a
bounded command-handler API. Those layers will build on this client rather than
introducing application-specific behavior.

The current immutable pin is deliberately marked `releaseReady: false` because
the registry has no stable calendar release of `@openclaw/gateway-protocol` yet
and the pinned beta does not publish every required node event payload.

Run `bash scripts/verify-protocol-pin.sh` to download the pinned npm tarball,
verify its SHA-512 digest, and confirm its version, schema identity, definition
count, and protocol-level constants. Vendoring the complete schema remains an
R1 exit requirement once a stable package artifact is available.

## License

[MIT](LICENSE)
