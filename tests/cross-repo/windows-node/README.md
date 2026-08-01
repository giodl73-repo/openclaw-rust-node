# Windows node composition proof

This proof composes an exact validated shared-runtime revision with an exact
Windows adopter revision. It verifies Git blobs rather than working-tree bytes
so normal CRLF checkout conversion cannot masquerade as contract drift.

The harness requires:

- the candidate shared-runtime checkout to contain the pinned green revision
  without subsequent changes under `crates/` or `test/fixtures/`;
- the Windows checkout to be exactly the pinned adopter revision;
- both consumed implementation/test surfaces to be free of uncommitted changes;
- all three sidecar fixture blobs to equal their recorded OpenClaw-owned blob;
- the full shared Rust workspace tests to pass; and
- the focused Windows `RustSidecar` conformance tests to pass.

Run from PowerShell:

```powershell
./tests/cross-repo/windows-node/run.ps1 `
  -RustNodeRoot C:\src\openclaw-rust-node `
  -WindowsNodeRoot C:\src\openclaw-windows-rust-sidecar
```

Use `-SkipTests` only to audit revision and fixture pins. A complete evidence
run omits that switch.

This is contract composition evidence. It does not launch a Rust process,
select the Rust runtime in Windows, provide protected bootstrap or concrete
IPC, or prove production rollout and rollback.
