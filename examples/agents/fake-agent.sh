#!/bin/sh
# Deterministic fake agent for examples and evidence. Reads the prompt from
# stdin, writes a report into the isolated workspace, and returns JSON with a
# score so cycle/until examples can consume it.
set -eu
prompt=$(cat)
mkdir -p reports
{
    echo "# Review"
    echo
    echo "The agent received this prompt:"
    echo
    echo '```'
    echo "$prompt"
    echo '```'
} >reports/review.md
printf '{"score":0.92,"summary":"review complete","artifact":"reports/review.md"}\n'
