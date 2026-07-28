# Contributing

This project is currently in its architecture and conformance phase.

Useful contributions include:

- verifying OpenClaw Gateway protocol behavior;
- identifying reusable Rust code in existing OpenClaw clients;
- proposing language-neutral conformance fixtures;
- reviewing credential storage, reconnect, cancellation, and execution-policy
  requirements;
- testing the smallest vertical client slice against real Gateway releases.

Please avoid adding new wire fields or product-specific commands here. Gateway
protocol and node-command changes belong in
[`openclaw/openclaw`](https://github.com/openclaw/openclaw) first.
