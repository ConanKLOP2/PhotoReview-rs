#!/usr/bin/env bash
# Build <worktree>/.codex-pack.md for one unit so Codex needs no exploration:
# contract + plan row + C# sources + pub signatures of Depends-on units' owned files + CODEX-RULES.md.
# Usage: codex-pack.sh <unit e.g. W8-04a> <worktree-dir>     (warns on stderr when the pack is > ~60 KB)
set -u
U="${1:?unit}"; WT="${2:?worktree}"
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="python $SELF/lib/codex-lib.py"
PLAN_REL="${PLAN_REL:-docs/WAVE-8-PLAN.md}"
CS_REPO="${CS_REPO:-C:/MyProjects/PhotoReview}"
pick() { [ -f "$WT/$1" ] && echo "$WT/$1" || echo "$SELF/../../$1"; }
CONTRACT="$(pick contracts/$U.md)"; PLAN="$(pick "$PLAN_REL")"
[ -f "$CONTRACT" ] || { echo "codex-pack: no contract for $U" >&2; exit 1; }
BASE="$($LIB baseline "$CONTRACT" | tr -d '\r')"; BASE="${BASE:-${CS_BASELINE:-d92d6aae}}"
csgit() { git -c safe.directory="$CS_REPO" -C "$CS_REPO" "$@"; }
OUT="$WT/.codex-pack.md"
LS="$(mktemp)"; csgit ls-tree -r --name-only "$BASE" > "$LS" 2>/dev/null
{
  echo "# Pack for $U (generated, read-only context; baseline C# $BASE)"
  echo; echo "## Plan row"; $LIB row "$PLAN" "$U" | tr -d '\r'
  echo; echo "## Contract ($U)"; echo; tr -d '\r' < "$CONTRACT"
  echo; echo "## C# source files (git show $BASE:<path>)"
  for t in $($LIB cs "$CONTRACT" | tr -d '\r'); do
    p=""
    if grep -qxF "$t" "$LS"; then p="$t"; else
      b="$(basename "$t")"; p="$(grep -F "/$b" "$LS" | awk -F/ -v b="$b" '$NF==b' | head -1)"
    fi
    if [ -z "$p" ]; then echo; echo "### $t: NOT FOUND at $BASE (use the rules' git show command if you need it)"; continue; fi
    echo; echo "### $p"; echo '```csharp'; csgit show "$BASE:$p" 2>/dev/null | tr -d '\r'; echo '```'
  done
  echo; echo "## Public signatures of the units this unit depends on (from this worktree)"
  for d in $($LIB deps "$PLAN" "$U" | tr -d '\r'); do
    [ -f "$WT/contracts/$d.md" ] || continue
    for f in $($LIB owned "$WT/contracts/$d.md" | tr -d '\r' | grep '/src/.*\.rs$'); do
      [ -f "$WT/$f" ] || continue
      echo; echo "### $d: $f"; echo '```rust'
      grep -nE '^\s*(pub |impl\b|#\[derive)' "$WT/$f" | head -200; echo '```'
    done
  done
  echo; echo "## CODEX-RULES.md"; echo; tr -d '\r' < "$SELF/CODEX-RULES.md"
} > "$OUT"
rm -f "$LS"
SZ=$(wc -c < "$OUT")
[ "$SZ" -gt 61440 ] && echo "codex-pack: WARNING $U pack is $SZ bytes (> ~60 KB)" >&2
echo "codex-pack: $OUT ($SZ bytes)"
