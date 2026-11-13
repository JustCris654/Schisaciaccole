#!/usr/bin/env bash
# PostToolUse hook: rustfmt a just-edited .rs file with its crate's edition.
set -u

input=$(cat) || exit 0
file=$(printf '%s' "$input" | jq -r '.tool_input.file_path // empty' 2>/dev/null) || exit 0
case "$file" in
  *.rs) ;;
  *) exit 0 ;;
esac
[ -f "$file" ] || exit 0

edition=2021
dir=$(dirname "$file")
while [ "$dir" != "/" ] && [ -n "$dir" ]; do
  if [ -f "$dir/Cargo.toml" ]; then
    found=$(sed -n 's/^edition *= *"\([0-9]*\)".*/\1/p' "$dir/Cargo.toml" | head -n1)
    if [ -n "$found" ]; then
      edition=$found
      break
    fi
  fi
  dir=$(dirname "$dir")
done

rustfmt --edition "$edition" "$file" >/dev/null 2>&1 || true
exit 0
