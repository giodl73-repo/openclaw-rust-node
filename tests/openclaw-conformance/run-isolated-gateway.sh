#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
openclaw_checkout="${OPENCLAW_CHECKOUT:?OPENCLAW_CHECKOUT must name a built OpenClaw checkout}"
openclaw_cli="$openclaw_checkout/openclaw.mjs"

if [[ ! -f "$openclaw_cli" ]]; then
  echo "OpenClaw CLI not found at $openclaw_cli" >&2
  exit 1
fi
if [[ ! -f "$openclaw_checkout/dist/entry.js" && ! -f "$openclaw_checkout/dist/entry.mjs" ]]; then
  echo "OpenClaw checkout must be built before running conformance" >&2
  exit 1
fi

test_root="$(mktemp -d)"
gateway_log="$test_root/gateway.log"
gateway_pid=""

cleanup() {
  if [[ -n "$gateway_pid" ]] && kill -0 "$gateway_pid" 2>/dev/null; then
    kill "$gateway_pid" 2>/dev/null || true
    wait "$gateway_pid" 2>/dev/null || true
  fi
  rm -rf "$test_root"
}
trap cleanup EXIT INT TERM

mkdir -p "$test_root/home" "$test_root/state"
export HOME="$test_root/home"
export OPENCLAW_STATE_DIR="$test_root/state"
export OPENCLAW_CONFIG_PATH="$test_root/state/openclaw.json"
export OPENCLAW_GATEWAY_TOKEN="$(openssl rand -hex 32)"
export OPENCLAW_GATEWAY_PORT="$(node -e 'const net=require("node:net");const server=net.createServer();server.listen(0,"127.0.0.1",()=>{console.log(server.address().port);server.close();});')"
export OPENCLAW_GATEWAY_URL="ws://127.0.0.1:$OPENCLAW_GATEWAY_PORT"
export OPENCLAW_CLI="$openclaw_cli"

node "$openclaw_cli" gateway run \
  --bind loopback \
  --port "$OPENCLAW_GATEWAY_PORT" \
  --auth token \
  --allow-unconfigured >"$gateway_log" 2>&1 &
gateway_pid="$!"

ready=false
for _ in $(seq 1 120); do
  if ! kill -0 "$gateway_pid" 2>/dev/null; then
    break
  fi
  if (echo >"/dev/tcp/127.0.0.1/$OPENCLAW_GATEWAY_PORT") 2>/dev/null; then
    ready=true
    break
  fi
  sleep 0.25
done

if [[ "$ready" != true ]]; then
  echo "isolated OpenClaw Gateway did not become ready" >&2
  tail -n 120 "$gateway_log" >&2 || true
  exit 1
fi

if ! cargo test \
  --manifest-path "$repo_root/tests/openclaw-conformance/Cargo.toml" \
  --locked --test live_gateway -- --ignored --nocapture; then
  echo "isolated Gateway log tail:" >&2
  tail -n 120 "$gateway_log" >&2 || true
  exit 1
fi
