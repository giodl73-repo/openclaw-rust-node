# OpenClaw Rust Node

Experimental Rust node client and headless node host for the
[OpenClaw](https://github.com/openclaw/openclaw) Gateway protocol.

> [!IMPORTANT]
> This is an early design and conformance project. It is not currently an
> official OpenClaw component, does not yet provide a working node client, and
> makes no compatibility guarantee.

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
six-PR implementation plan, security requirements, and evidence gates. The
earlier [proposal](docs/proposal.md) remains as a shorter discussion brief.

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

## Current phase

1. Review [RFC 0001](rfcs/0001-rust-node-client-and-host.md) with OpenClaw
   maintainers.
2. Establish the protocol-schema pin and conformance workflow.
3. Build one minimal client that pairs and handles a harmless read-only
   command.
4. Compare behavior against the TypeScript reference node host.

## License

[MIT](LICENSE)
