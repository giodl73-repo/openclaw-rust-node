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

## Current phase

1. Review [RFC 0001](rfcs/0001-rust-node-client-and-host.md) with OpenClaw
   maintainers.
2. Resolve [OpenClaw #115375](https://github.com/openclaw/openclaw/issues/115375)
   for the missing node cancellation contract, released node-contract
   projection, and fixtures.
3. Land the protocol pin and transport state machine in separately reviewable
   PRs.
4. Build the smallest evidence slice: device pairing, separate node capability
   approval, and one bounded namespaced status command.
5. Stop for a cost and ownership review before streaming invocation, packaging,
   built-in commands, or adopter-specific work.

## License

[MIT](LICENSE)
