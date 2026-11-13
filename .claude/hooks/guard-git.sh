#!/usr/bin/env bash
# PreToolUse hook: request confirmation for ordinary git push commands.
set -u

input=$(cat) || exit 0
cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty' 2>/dev/null) || exit 0
[ -n "$cmd" ] || exit 0

# Permission settings remain the primary control; this is not a shell parser.
printf '%s' "$cmd" | grep -Eq '(^|[;&|[:space:]])git[[:space:]]+push([[:space:]]|$)' || exit 0

cat <<'JSON'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"Git pushes require an explicit developer request under CLAUDE.md. Confirm that the requested push is authorized."}}
JSON
exit 0
