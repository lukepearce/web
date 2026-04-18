#!/usr/bin/env bash
# ccm-statusline.sh
#
# Claude Code statusLine hook that:
#   1. Computes context % from the active session's transcript file.
#   2. Writes a sidecar JSON for ccm to read at
#      $XDG_CACHE_HOME/ccm/panes/$TMUX_PANE.json
#   3. Prints a one-line status for Claude Code to render at the bottom.
#
# Wire into ~/.claude/settings.json:
#
#   {
#     "statusLine": {
#       "type": "command",
#       "command": "/full/path/to/ccm-statusline.sh"
#     }
#   }
#
# Requires: jq

set -euo pipefail

INPUT="$(cat)"

TRANSCRIPT=$(printf '%s' "$INPUT" | jq -r '.transcript_path // empty')
MODEL_ID=$(printf '%s' "$INPUT" | jq -r '.model.id // ""')
MODEL_NAME=$(printf '%s' "$INPUT" | jq -r '.model.display_name // .model.id // "claude"')
COST_USD=$(printf '%s' "$INPUT" | jq -r '.cost.total_cost_usd // 0')

# Context window per model (rough; tweak as needed).
case "$MODEL_ID" in
  *opus*|*sonnet*|*haiku*) CONTEXT_MAX=200000 ;;
  *) CONTEXT_MAX=200000 ;;
esac

CONTEXT_TOKENS=0
if [ -n "$TRANSCRIPT" ] && [ -f "$TRANSCRIPT" ]; then
  # Find the most recent assistant turn with a usage block; sum its token
  # counts. This approximates the live context size.
  CONTEXT_TOKENS=$(
    tac "$TRANSCRIPT" 2>/dev/null \
      | jq -r 'select(.message.usage) | .message.usage
              | (.input_tokens // 0)
              + (.cache_read_input_tokens // 0)
              + (.cache_creation_input_tokens // 0)
              + (.output_tokens // 0)' \
      | head -1 || true
  )
  CONTEXT_TOKENS=${CONTEXT_TOKENS:-0}
fi

CONTEXT_PCT=0
if [ "$CONTEXT_MAX" -gt 0 ] && [ "$CONTEXT_TOKENS" -gt 0 ]; then
  CONTEXT_PCT=$(( CONTEXT_TOKENS * 100 / CONTEXT_MAX ))
fi

# Write sidecar so ccm can pick it up.
if [ -n "${TMUX_PANE:-}" ]; then
  CCM_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/ccm/panes"
  mkdir -p "$CCM_DIR"
  TS=$(date +%s)
  cat > "$CCM_DIR/${TMUX_PANE}.json" <<JSON
{
  "model": "$MODEL_ID",
  "context_pct": $CONTEXT_PCT,
  "context_tokens": $CONTEXT_TOKENS,
  "context_max": $CONTEXT_MAX,
  "cost_usd": $COST_USD,
  "ts": $TS
}
JSON
fi

# What Claude Code shows at the bottom of its UI.
printf "%s · %d%% ctx · \$%.2f" "$MODEL_NAME" "$CONTEXT_PCT" "$COST_USD"
