# Cross-repository evidence

This directory holds black-box tests and redacted evidence that compose an
exact shared Rust runtime revision with an exact product-adapter revision.

The harness may launch both sides, but it owns neither implementation. Each
proof must pin both revisions, use isolated state and injected test
credentials, and report accepted behavior plus at least one structured failure.

Suitable proofs include pairing and capability approval, invocation,
readiness, reconnect, cancellation, bounded overload, process supervision,
upgrade, and rollback. Product source code belongs in its integration area;
generic runtime tests and fixtures belong beside the shared crates.

## Shared-runtime live Gateway proof

The first proof is deliberately product-neutral and lives in a standalone
package that depends on the synchronized node-host crate by path:

```text
tests/openclaw-conformance/tests/live_gateway.rs
```

This preserves the byte-exact OpenClaw snapshot while the test incubates. The
test is compiled by the ordinary validation gate and ignored at execution time
unless the operator supplies an isolated real Gateway and CLI. It proves:

- Gateway authentication and challenge-bound node identity;
- Gateway-owned command-surface approval, accepting either an explicit pending
  request or OpenClaw's trusted first-pair automatic approval;
- successful and structured-failure `node.invoke` paths;
- reconnect with the Gateway-issued device token; and
- fail-closed classification of an invalid device token.

Run it only against disposable Gateway state:

```console
OPENCLAW_GATEWAY_URL=ws://127.0.0.1:<port> \
OPENCLAW_GATEWAY_TOKEN=<ephemeral-test-token> \
OPENCLAW_CLI=/absolute/path/to/openclaw.mjs \
cargo test --manifest-path tests/openclaw-conformance/Cargo.toml \
  --locked --test live_gateway -- --ignored --nocapture
```

On Windows, set the same process-local environment variables in PowerShell;
`OPENCLAW_NODE_EXE` may override the default `node` executable. Do not point
this proof at a normal user Gateway or persist its generated node identity.

Product-adapter tests should invoke this shared proof or consume its result;
they should not copy its Gateway and runtime logic into the adopter tree.
