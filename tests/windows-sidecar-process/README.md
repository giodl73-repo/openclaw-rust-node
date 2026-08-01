# Windows sidecar process probe

This test-only executable proves that the shared Rust sidecar implementation can
exchange real length-prefixed, authenticated frames with the Windows supervisor
over redirected standard input and output.

It is intentionally not a product host. The Windows test owns process launch and
injects a one-session bootstrap key through the child environment. Standard output
is reserved for protocol frames; diagnostics use standard error.
