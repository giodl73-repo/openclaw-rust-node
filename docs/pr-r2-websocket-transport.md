# R2 Draft: WebSocket Transport State Machine

Base this PR on R1 only after the protocol-pin boundary is accepted or while R1
is the sole short-lived dependency.

## Review scope

- stopped, connecting, ready, backoff, and authentication-paused states;
- challenge-before-connect ordering and protocol-range enforcement;
- request correlation, timeout and close cleanup, and bounded reconnect delay;
- monotonically increasing transport generations that reject retired sockets;
- loopback-only plaintext and remote TLS requirements;
- deterministic scripted-Gateway tests.

## Explicit exclusions

- persistent identity or device tokens;
- device or node-capability pairing;
- command advertisement or invocation;
- cancellation, progress, pending work, or `system.*` commands.

## Required proof

The receive loop remains non-blocking, pending requests are failed exactly once
on disconnect, retired sockets cannot mutate current state, and terminal
authentication/protocol errors do not reconnect forever.

Primary analogues are `@openclaw/gateway-client`, Swift
`GatewayNodeSession`, the ESP runtime/transport split, and the Linux Rust
operator transport. None is copied as a wire-contract source.
