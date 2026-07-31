# Repository layout and synchronization contract

This repository incubates a single reusable OpenClaw Rust runtime and the
adapters that prove it can be embedded in real products. Its directory
boundaries are also contribution boundaries: a change should have one clear
owner and one clear destination.

## Layout

```text
openclaw-rust-node/
├── crates/
│   ├── openclaw-gateway-client/
│   └── openclaw-node-host/
├── test/fixtures/
├── integrations/
│   └── edge-chromium/
├── tests/
│   └── cross-repo/
└── src/, protocol/, fixtures/, examples/, rfcs/
```

The root single-crate implementation and its supporting directories are the
historical prototype and package-evidence source. New product integration must
use `crates/`; the prototype is not another supported runtime.

## Ownership and round trips

| Area | Owns | Round-trips with | Must not contain |
| --- | --- | --- | --- |
| `crates/` | Generic Gateway session, node lifecycle, bounded runtime, foreground host, and sidecar contracts | `openclaw/openclaw` | Microsoft product names, platform deployment, native command implementations, product policy |
| `test/fixtures/` | Language-neutral wire and lifecycle conformance vectors consumed by the shared crates | `openclaw/openclaw` | Product-only payloads or credentials |
| `integrations/edge-chromium/` | Edge/Chromium adapters, local IPC selection, credential-store binding, process supervision, native handlers, telemetry, rollout, and rollback | Microsoft Edge/Chromium repositories | Forked Gateway protocol, copied runtime logic, changes to OpenClaw command semantics |
| `tests/cross-repo/` | Black-box proof across a pinned shared runtime and a product adapter | Evidence may inform both organizations | Production implementation or independent protocol authority |

OpenClaw remains authoritative for Gateway schemas, authentication, pairing,
node capability approval, command semantics, and shared conformance. Microsoft
remains authoritative for the Copilot application shell, Edge/Chromium
packaging, platform credential storage, local IPC, service supervision,
Windows-native tools, telemetry, and fleet rollout.

## Dependency direction

```text
Edge/Chromium product shell
          |
          v
Edge/Chromium adapter and native handlers
          |
          v
openclaw-node-host
          |
          v
openclaw-gateway-client
          |
          v
OpenClaw Gateway contract
```

Dependencies point toward the shared crates. Shared crates never import or
conditionally compile Edge/Chromium product code. Cross-repository tests may
compose both sides, but neither production side depends on the test harness.

## Incubation and consumption

Until OpenClaw selects an official crate distribution mechanism,
Edge/Chromium integrations may consume an exact reviewed source snapshot from
this repository. Every such snapshot must record:

1. the exact OpenClaw source commit and PR stack;
2. the exact private-repository commit consumed by the adopter;
3. the conformance fixtures and tests that passed;
4. any downstream-only adapter changes;
5. the upgrade and rollback procedure.

Do not patch a copied crate only in an Edge/Chromium tree. A generic runtime
fix starts in the OpenClaw-facing `crates/` area, is reviewed upstream, and is
then synchronized into adopters. An urgent downstream mitigation may pin or
disable a feature, but its generic fix must still return through the shared
area.

When OpenClaw publishes or otherwise supports the crates, replace source-drop
consumption with the accepted pinned mechanism. The adapter boundary should
remain unchanged.

## Change routing

Route a change by asking who owns the behavior:

- Gateway framing, TLS, identity, reconnect, node invocation, bounded runtime,
  generic plugin/MCP/skill node-mode support, or language-neutral fixtures:
  change `crates/` or `test/fixtures/` and propose it to OpenClaw.
- Windows or Edge process startup, credential storage, named pipe or Chromium
  IPC, native tools, telemetry, installer behavior, feature selection, or
  rollback: change `integrations/edge-chromium/` and propose it internally.
- Proof that a particular runtime revision works with a particular adapter:
  change `tests/cross-repo/`, pin both revisions, and publish only redacted
  evidence.

If a change appears to require edits on both production sides, split it into a
generic contract/runtime change and an adopter change. Land or pin the generic
side first, then update the adapter. This keeps each review independently
reversible and prevents the private repository from becoming a second
protocol owner.

## Secrets and evidence

No area stores Gateway tokens, device identity secrets, signing material,
private endpoints, or unredacted product logs. Test deployment uses isolated
state and injected credentials. Cross-repository evidence records versions,
commands, outcomes, and sanitized diagnostics, never credential values.
