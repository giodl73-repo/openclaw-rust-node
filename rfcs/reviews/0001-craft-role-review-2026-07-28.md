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
3. The generic host at R7, with optional `system.which` parity at R9, is a valid
   terminal product. `system.run` is not attempted without an upstream-owned
   execution-policy corpus.

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

10. Nothing is published before the R8 packaging and experimental-release gate.
11. Signed binaries are mandatory; missing signing infrastructure omits that
    platform artifact.
12. The initial release remains one crate and one binary. Any later multi-crate
    release requires coordinated ordering and a tested yank/recovery runbook.
13. Fresh-environment install smoke, artifact smoke, provenance, SBOM,
    dependency audit, support/EOL, and rollback are release gates.
14. R7 proves foreground operation, structured logs, health/readiness, and
    graceful shutdown; R8 separately reviews systemd, launchd, Windows Service,
    platform credential permissions, signing, and artifacts.

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

## Follow-up source audit

A current-source audit at OpenClaw `4683c752` added five PR 0 requirements:

1. Maintain a node-contract manifest and upstream missing published contracts,
   beginning with the locally coerced `node.invoke.cancel` payload.
2. Model device pairing and node capability approval as separate states and
   prove their distinct request IDs and reconnect behavior.
3. Treat manual approval as the V1 enrollment baseline until the standalone
   binary supplies a compatible `openclaw node identity --json` shim or
   OpenClaw accepts a configurable SSH identity-probe contract.
4. Reserve OpenClaw-owned command namespaces and treat `system.which` as
   admin-sensitive rather than a harmless first command.
5. Report the advertised protocol range, server protocol, and legacy node
   compatibility-window status instead of claiming a negotiated dialect.

These requirements were incorporated into RFC 0001 before implementation.

## PR-plan audit

A second current-source audit at OpenClaw
`b07efcb6dc7e84451b8421637f412df4de4a52ab` changed the delivery plan without
changing the ownership thesis:

1. The public `@openclaw/gateway-client` is the behavioral analogue for
   challenge ordering, request correlation, timeouts, reconnect, sequence gaps,
   and host-owned transport dependencies. Transport is now its own Rust PR.
2. Identity/authentication/device pairing and node capability approval are
   separate PRs because they cross different trust and review boundaries.
3. The former invocation PR is split into request/result core and the streaming
   input/progress/cancellation lifecycle. The latter waits for a released
   cancellation schema and fixtures from OpenClaw.
4. Pending work and dynamic plugin tool/skill publication are removed from V1;
   the first bounded custom command does not depend on them.
5. The generic headless binary, release packaging, and admin-sensitive
   `system.which` parity are independently reviewable PRs.
6. `system.run` is a post-V1 program rather than the last member of the routine
   implementation stack.

The merge policy is merge-as-you-go, with at most one short-lived dependent PR
stacked during review latency and no stacking across unresolved schema,
ownership, security, or release gates.
