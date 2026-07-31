# Edge/Chromium integration

This directory is the staging area for Microsoft-owned product integration
around the shared Rust crates in [`../../crates/`](../../crates/).

Appropriate contents include:

- process supervision and verified artifact startup;
- protected bootstrap and platform credential-store bindings;
- concrete local IPC adapters;
- Windows- and Edge/Chromium-native command handlers;
- product telemetry and audit projection;
- feature selection, deployment, upgrade, rollback, and coexistence with an
  incumbent runtime.

This directory must not copy or fork the Gateway client, node runtime, Gateway
schemas, or language-neutral conformance fixtures. Generic fixes go through
the OpenClaw-facing shared areas first and are then consumed here at a pinned
revision.

Code in this directory is expected to round-trip with the appropriate private
Edge/Chromium repository. Keep private repository identifiers, credentials,
and unredacted logs out of the OpenClaw-facing shared areas and committed test
evidence.
