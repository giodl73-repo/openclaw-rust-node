# RFC 0001 Role Review

Date: 2026-07-28  
RFC: `rfcs/0001-rust-node-client-and-host.md`

## Review lenses

- Gateway/runtime owner
- Security owner
- Open-source community steward
- Platform and release operator

## Source verification

Two review passes used stale OpenClaw source and produced factual findings that
were rejected after checking live `openclaw/openclaw` main:

- `@openclaw/gateway-protocol` is public, versioned, and includes generated
  `protocol.schema.json` in its npm package.
- `PROTOCOL_VERSION` is 4 and `MIN_NODE_PROTOCOL_VERSION` is 3, so authenticated
  nodes have an explicit N-1 rolling-upgrade window.
- current node invocation supports progress, ordered input, and cancellation in
  addition to request/result.

Those claims remain grounded in OpenClaw's published package and source rather
than in a local checkout.

## Accepted findings

### Governance and ecosystem

1. PR 0 now records an explicit accepted-versus-independent outcome. If
   OpenClaw maintainers decline ownership, the project remains experimental,
   uses no official-looking published artifact names, and makes no upstream
   compatibility guarantee.
2. Public crate and binary names are gated on ownership acceptance.
3. The read-only host at PR 4 is a valid terminal product. `system.run` is not
   attempted without an upstream-owned execution-policy corpus.

### Runtime contract

4. Idempotency keys are optional on the node-facing event and handlers must
   degrade safely when no key is present.
5. Pending-work drain/ack support must be implemented or excluded through the
   advertised capability surface.
6. Gateway removal and credential revocation are authoritative: disconnect,
   failure of in-flight work, refusal of new work, and no automatic re-pairing.

### CI and compatibility

7. Release gates run against immutable OpenClaw releases. A separate
   non-gating `main` canary detects future drift.
8. The compatibility table maps Rust release, OpenClaw release, wire protocol,
   and MSRV.
9. Gateway integration fixtures must be reproducible from pinned artifacts.

### Publishing and operations

10. Nothing is published before PR 4.
11. Signed binaries are mandatory; missing signing infrastructure omits that
    platform artifact.
12. The initial release remains one crate and one binary. Any later multi-crate
    release requires coordinated ordering and a tested yank/recovery runbook.
13. Fresh-environment install smoke, artifact smoke, provenance, SBOM,
    dependency audit, support/EOL, and rollback are release gates.
14. PR 4 includes systemd, launchd, Windows Service, platform credential
    permissions, structured logs, health/readiness, and graceful shutdown.

## Findings redirected to OpenClaw ownership

The security review proposed fleet quarantine, tamper-evident SIEM audit,
credential-custody reporting fields, and new fail-closed governance semantics.
Those may be useful OpenClaw features, but they cannot be invented by a Rust
client without forking node behavior.

RFC 0001 therefore requires parity with existing OpenClaw approval, audit,
revocation, and emergency-disable contracts. Any missing contract must land in
OpenClaw before Rust consumes it.

## Remaining decisions

- official repository and maintainers;
- public crate and binary names;
- first read-only command;
- execution-policy corpus owner and location;
- emergency disable path for a faulty execution release;
- exact support/EOL window.

## Verdict

The architecture is suitable for continued RFC review. The core design remains
unchanged: OpenClaw owns the protocol and policy; Rust provides a bounded,
conformant client and host. The accepted revisions make ownership, publication,
rollback, and compatibility promises auditable before implementation begins.
