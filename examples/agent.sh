#!/bin/sh
# Minimal agent provider for the examples. Reads the prompt on stdin and emits
# a JSON result with a score and an improvement proposal.
cat >/dev/null
cat <<'JSON'
{
  "score": 0.93,
  "diagnosis": "The plan is sound; keep the scope small and reversible.",
  "changes": ["no structural change"],
  "params_patch": {"goal": "ship the improved release"},
  "next_prompt": "Re-run with the improved goal and compare scores.",
  "confidence": 0.9
}
JSON
