#!/usr/bin/env bash
# One-call status for a loop tick: open PRs, integration branch heads, newest workflow progress, Codex quota.
# Usage: tools/autopilot/status.sh [integration-branch=integration/wave-8]
R=C:/MyProjects/PhotoReview-rs; IB="${1:-integration/wave-8}"; cd "$R" || exit 1
echo "== PRs"; gh pr list --json number,title,mergeStateStatus --jq '.[]|"\(.number) \(.title) \(.mergeStateStatus)"' 2>&1 | head -10
echo "== master"; git fetch -q 2>/dev/null; git log --oneline -1 origin/master | cut -c1-90
echo "== $IB"; git log --oneline -1 "$IB" 2>/dev/null | cut -c1-90
WF=$(ls -dt "$HOME"/.claude/projects/*/*/subagents/workflows/wf_* 2>/dev/null | head -1)
if [ -n "$WF" ]; then echo "== newest workflow $(basename $WF): results=$(wc -l < $WF/journal.jsonl) last_log=$(ls -lt --time-style=+%H:%M $WF | sed -n 2p | awk '{print $6}') now=$(date +%H:%M)"; fi
echo "== codex"; bash tools/autopilot/codex-usage.sh 99 2>&1 | head -2
echo "== stray"; git status --short | grep -v '^??.*target' | head -5
