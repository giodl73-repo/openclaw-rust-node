# Windows sidecar process probe

This test-only executable proves that the shared Rust sidecar implementation can
exchange real length-prefixed, authenticated frames with the Windows supervisor
over redirected standard input and output.

It is intentionally not a product host. The Windows test owns process launch,
verifies an exact SHA-256 artifact pin, and writes a bounded one-session
bootstrap record through the child's inherited private stdin pipe. No bootstrap
secret is placed in arguments, environment variables, logs, or files. The child
computes its own executable hash and presents that identity in the authenticated
handshake. Standard output is reserved for protocol frames; diagnostics use
standard error.
