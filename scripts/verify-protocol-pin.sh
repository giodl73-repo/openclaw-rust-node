#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest_path="$repo_root/protocol/node-contract.json"
pin_relative="$(jq -er '.protocolPin' "$manifest_path")"
pin_path="$(realpath -e "$repo_root/protocol/$pin_relative")"
pins_root="$(realpath -e "$repo_root/protocol/pins")"
case "$pin_path" in
  "$pins_root"/*) ;;
  *)
    echo "manifest protocolPin must resolve under protocol/pins: $pin_relative" >&2
    exit 1
    ;;
esac
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

version="$(jq -er '.version' "$pin_path")"
tarball_url="$(jq -er '.tarball' "$pin_path")"
sha512_hex="$(jq -er '.sha512Hex' "$pin_path")"
schema_id="$(jq -er '.schemaId' "$pin_path")"
definition_count="$(jq -er '.schemaDefinitionCount' "$pin_path")"
protocol_version="$(jq -er '.protocolVersion' "$pin_path")"
minimum_node_protocol_version="$(jq -er '.minimumNodeProtocolVersion' "$pin_path")"

curl --fail --location --silent --show-error "$tarball_url" --output "$temp_dir/package.tgz"
printf '%s  %s\n' "$sha512_hex" "$temp_dir/package.tgz" | sha512sum --check --status
tar -xzf "$temp_dir/package.tgz" -C "$temp_dir"

test "$(jq -er '.version' "$temp_dir/package/package.json")" = "$version"
test "$(jq -er '.["$id"]' "$temp_dir/package/protocol.schema.json")" = "$schema_id"
test "$(jq -er '.definitions | length' "$temp_dir/package/protocol.schema.json")" = "$definition_count"
while IFS= read -r definition; do
  if ! jq -e --arg definition "$definition" \
    '.definitions[$definition] != null' \
    "$temp_dir/package/protocol.schema.json" >/dev/null; then
    echo "published manifest definition is absent from pinned schema: $definition" >&2
    exit 1
  fi
done < <(jq -er '.contracts[] | select(.status == "published") | .schemaDefinition' "$manifest_path")
grep -Fq "const PROTOCOL_VERSION = $protocol_version;" "$temp_dir/package/dist/version.mjs"
grep -Fq \
  "const MIN_NODE_PROTOCOL_VERSION = $minimum_node_protocol_version;" \
  "$temp_dir/package/dist/version.mjs"

echo "verified @openclaw/gateway-protocol@$version"
