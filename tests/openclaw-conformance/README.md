# OpenClaw conformance

This standalone test package validates the exact synchronized crates under
`../../crates/` without modifying that OpenClaw snapshot. Its tests and
operator harness are product-neutral and are candidates to round-trip
upstream.

The ignored real-Gateway test proves authentication, challenge-bound node
identity, command approval, invocation success and structured failure,
device-token reconnect, and invalid-token failure classification.

OpenClaw may approve a trusted first node's initial capability surface during
the pairing step. The proof reports whether approval was explicit or trusted
automatic, then requires a device-token reconnect before invoking against the
approved pairing generation. A future negative fixture should disable trusted
auto-approval to prove the pending and filtered path independently.

Compile the harness without external services:

```console
cargo test --manifest-path tests/openclaw-conformance/Cargo.toml --locked
```

Run the complete proof on Linux with a built OpenClaw source checkout:

```console
OPENCLAW_CHECKOUT=/absolute/path/to/openclaw \
bash tests/openclaw-conformance/run-isolated-gateway.sh
```

The runner creates disposable Gateway state and credentials, selects a free
loopback port, applies the same minimal channel/provider environment as
OpenClaw's process-level E2E helper, stops the Gateway on exit, and prints
sanitized failure logs. It must not be pointed at a normal user Gateway.

The disposable configuration explicitly allowlists only the two neutral
conformance commands. OpenClaw otherwise filters custom commands out of the
approval surface, so the proof preserves the production fail-closed policy
while exercising the operator opt-in and approval path.
