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
