#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SBE_TOOL_JAR="${SBE_TOOL_JAR:-}"

if [[ -z "$SBE_TOOL_JAR" ]]; then
  echo "Set SBE_TOOL_JAR to the official aeron-io/simple-binary-encoding sbe-all jar." >&2
  echo "Example: SBE_TOOL_JAR=/path/to/sbe-all-1.38.0-SNAPSHOT.jar $0" >&2
  exit 1
fi

# sbetool writes a whole crate (<output>/nanofix_sbe/{Cargo.toml,src}).
# Keep the hand-maintained Cargo.toml in crates/nanofix-sbe and take only src/.
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

java --add-opens java.base/jdk.internal.misc=ALL-UNNAMED \
  -Dsbe.output.dir="$OUT" \
  -Dsbe.target.language=Rust \
  -Dsbe.target.namespace=nanofix_sbe \
  -jar "$SBE_TOOL_JAR" \
  "$ROOT_DIR/crates/nanofix-sbe/schema/fix_aeron_envelope.xml"

rm -rf "$ROOT_DIR/crates/nanofix-sbe/src"
cp -R "$OUT/nanofix_sbe/src" "$ROOT_DIR/crates/nanofix-sbe/src"
