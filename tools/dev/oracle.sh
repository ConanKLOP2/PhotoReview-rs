#!/usr/bin/env bash
# Regenerate golden files with tools/oracle.ps1 several times and check they are reproducible.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GIT_BIN=${GIT_BIN:-git}; PS_BIN=${PS_BIN:-powershell.exe}
usage(){ echo 'usage: oracle.sh <command>... [--runs 2] [--commit <sha>] [--keep]' >&2; exit 2; }
runs=2; commit=""; keep=0; cmds=()
while [ $# -gt 0 ]; do
  case "$1" in
    --runs) [ $# -ge 2 ] || usage; runs=$2; shift 2;;
    --commit) [ $# -ge 2 ] || usage; commit=$2; shift 2;;
    --keep) keep=1; shift;;
    -*) usage;;
    *) cmds+=("$1"); shift;;
  esac
done
[ ${#cmds[@]} -gt 0 ] || usage
case "$runs" in ''|*[!0-9]*|0) echo "--runs must be a positive integer" >&2; exit 2;; esac
for c in "${cmds[@]}"; do case "$c" in *[!A-Za-z0-9._-]*|.*) echo "bad command name: $c" >&2; exit 2;; esac; done

golden="$ROOT/tests/golden"
tmp=$(mktemp -d) || exit 1
declare -A existed
restore(){
  [ "$keep" = 1 ] && return
  for c in "${cmds[@]}"; do
    if [ "${existed[$c]:-0}" = 1 ]; then cp "$tmp/committed/$c.json" "$golden/$c.json" 2>/dev/null
    elif [ "${created[$c]:-0}" = 1 ]; then rm -f "$golden/$c.json"; fi
  done
}
declare -A created
cleanup(){ restore; rm -rf "$tmp"; }
trap cleanup EXIT

# Warn when the native DLLs the oracle copies next to oracle-cs.exe are missing (oracle.ps1: turbojpeg.dll, libraw.dll).
common=$("$GIT_BIN" -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)
csrepo=${ORACLE_CSHARP_REPO:-}
[ -z "$csrepo" ] && [ -n "$common" ] && csrepo="$(dirname "$(dirname "$common")")/PhotoReview"
otemp=${ORACLE_TEMP:-${TEMP:-${TMP:-/tmp}}}
for dll in turbojpeg.dll libraw.dll; do
  found=0
  [ -n "$csrepo" ] && [ -f "$csrepo/native/x64/$dll" ] && found=1
  for f in "$otemp"/pr-oracle/*/core/"$dll"; do [ -f "$f" ] && found=1; done
  [ $found = 1 ] || echo "WARNING: $dll not found (looked in ${csrepo:-<C# repo unknown>}/native/x64 and $otemp/pr-oracle/*/core); native-decode commands will fail"
done

# Save the committed goldens first; refuse to overwrite local edits.
mkdir -p "$tmp/committed"
dirty=0
for c in "${cmds[@]}"; do
  if "$GIT_BIN" -C "$ROOT" show "HEAD:tests/golden/$c.json" > "$tmp/committed/$c.json" 2>/dev/null; then
    existed[$c]=1
    if [ -f "$golden/$c.json" ] && ! cmp -s "$tmp/committed/$c.json" "$golden/$c.json"; then
      echo "REFUSED: tests/golden/$c.json has local changes (commit or restore it first)"; dirty=1
    fi
  else
    existed[$c]=0; rm -f "$tmp/committed/$c.json"
    [ -f "$golden/$c.json" ] || created[$c]=1
  fi
done
[ $dirty = 0 ] || { keep=1; exit 1; }

if command -v cygpath >/dev/null 2>&1; then ps1=$(cygpath -w "$ROOT/tools/oracle.ps1"); else ps1="$ROOT/tools/oracle.ps1"; fi
declare -A failed
for ((r=1; r<=runs; r++)); do
  mkdir -p "$tmp/run$r"
  for c in "${cmds[@]}"; do
    args=(-NoProfile -ExecutionPolicy Bypass -File "$ps1")
    [ -n "$commit" ] && args+=(-Commit "$commit")
    args+=(-Command "$c")
    echo "run $r/$runs: $c"
    if ! "$PS_BIN" "${args[@]}"; then echo "FAIL run $r $c (oracle.ps1 exited non-zero)"; failed[$c]=1; continue; fi
    [ -f "$golden/$c.json" ] && { cp "$golden/$c.json" "$tmp/run$r/$c.json"; [ "${existed[$c]:-0}" = 1 ] || created[$c]=1; } || { echo "FAIL run $r $c (no tests/golden/$c.json produced)"; failed[$c]=1; }
  done
done

strip(){ grep -v '"csharp_commit"' "$1" | sha256sum | cut -d' ' -f1; }
rc=0
echo "--- result"
for c in "${cmds[@]}"; do
  if [ "${failed[$c]:-0}" = 1 ]; then echo "$c: FAILED"; rc=1; continue; fi
  first=$(sha256sum "$tmp/run1/$c.json" | cut -d' ' -f1); same=1
  for ((r=2; r<=runs; r++)); do [ "$(sha256sum "$tmp/run$r/$c.json" | cut -d' ' -f1)" = "$first" ] || same=0; done
  if [ $runs -lt 2 ]; then rep="single run (no reproducibility check)"
  elif [ $same = 1 ]; then rep="identical ($runs runs)"
  else rep="DIFFERENT between runs"; rc=1; fi
  if [ "${existed[$c]:-0}" != 1 ]; then cmp="no committed golden (new file)"
  elif cmp -s "$tmp/committed/$c.json" "$tmp/run1/$c.json"; then cmp="same as committed golden"
  elif [ "$(strip "$tmp/committed/$c.json")" = "$(strip "$tmp/run1/$c.json")" ]; then cmp="differs from committed golden ONLY in csharp_commit"
  else cmp="DIFFERS from committed golden"; fi
  echo "$c: $rep; $first; $cmp"
done
if [ "$keep" = 1 ]; then echo "--keep: regenerated tests/golden files left in place"; else echo "committed goldens restored"; fi
exit $rc
