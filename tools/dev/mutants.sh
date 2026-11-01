#!/usr/bin/env bash
# Time-boxed cargo-mutants run done the way Wave 8 taught us. Never kills processes by name.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CARGO_BIN=${CARGO_BIN:-cargo}; PY_BIN=${PY_BIN:-python}; TIMEOUT_BIN=${TIMEOUT_BIN:-timeout}
TMPBASE=${TEMP:-${TMPDIR:-/tmp}}
usage() { echo 'usage: mutants.sh <crate> <src-file>... [--tests a,b] [--native] [--cargo-arg A]... [--cargo-test-arg A]... [--budget SECONDS] [--jobs N] [--rerun-survivors] [-F REGEX]... [--skip REGEX]...
       mutants.sh --lint-args <raw cargo-mutants args...>   (only checks the libtest flag form, runs nothing)' >&2; exit 2; }

# lint_libtest <arg>...: the args are what cargo-mutants hands to `cargo test` (before the libtest separator). A libtest-only flag
# (--include-ignored, --test-threads, --nocapture, ...) in front of the first `--` makes cargo fail with "unexpected argument"; with
# `--baseline skip` cargo-mutants then counts every mutant as caught although no test ran (AUDIT-MUTATION-FLAG-2026-10-30).
# Returns 1 and prints the fix when it finds one.
lint_libtest() {
  local seen=0 a bad=0
  for a in "$@"; do
    if [ "$a" = -- ]; then seen=1; continue; fi
    [ "$seen" = 1 ] && continue
    case "$a" in
      --include-ignored|--ignored|--test-threads|--test-threads=*|--nocapture|--show-output|--exact|--skip|--skip=*|--list|--format|--format=*|--report-time|--shuffle)
        echo "BAD FLAG FORM: '$a' is a libtest flag but no '--cargo-test-arg=--' comes before it; cargo rejects it and every mutant is falsely reported caught." >&2
        bad=1;;
    esac
  done
  if [ "$bad" = 1 ]; then
    echo "Correct form: --cargo-test-arg=<filter> --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1  (or mutants.sh --native)" >&2
    return 1
  fi
  return 0
}
lint_raw_args() { # raw cargo-mutants command line tokens
  local l=() a after=0 want=0
  for a in "$@"; do
    if [ "$want" = 1 ]; then l+=("$a"); want=0; continue; fi
    if [ "$after" = 1 ]; then l+=("$a"); continue; fi
    case "$a" in
      --cargo-test-arg=*) l+=("${a#--cargo-test-arg=}");;
      --cargo-test-arg) want=1;;
      --) after=1;;
    esac
  done
  lint_libtest "${l[@]}"
}
if [ "${1:-}" = --lint-args ]; then shift; lint_raw_args "$@"; rc=$?; [ $rc -eq 0 ] && echo "flag form ok"; exit $rc; fi

mfilt=(); crate=""; files=(); tests=""; budget=1200; jobs=1; rerun=0; native=0; cargoargs=(); usertc=()
while [ $# -gt 0 ]; do
  case "$1" in
    --native) native=1; shift;;
    --cargo-arg) [ $# -ge 2 ] || usage; cargoargs+=("$2"); shift 2;;
    --cargo-test-arg) [ $# -ge 2 ] || usage; usertc+=("$2"); shift 2;;
    --tests) [ $# -ge 2 ] || usage; tests="$2"; shift 2;;
    --budget) [ $# -ge 2 ] || usage; budget="$2"; shift 2;;
    --jobs) [ $# -ge 2 ] || usage; jobs="$2"; shift 2;;
    --rerun-survivors) rerun=1; shift;;
    -F) [ $# -ge 2 ] || usage; mfilt+=(-F "$2"); shift 2;;   # cargo-mutants --re: only mutants whose name matches (regex, use . for spaces)
    --skip) [ $# -ge 2 ] || usage; mfilt+=(-E "$2"); shift 2;; # cargo-mutants --exclude-re: skip mutants whose name matches
    -*) usage;;
    *) if [ -z "$crate" ]; then crate="$1"; else files+=("$1"); fi; shift;;
  esac
done
[ -n "$crate" ] && [ ${#files[@]} -gt 0 ] || usage
case "$budget" in ''|*[!0-9]*) echo 'budget must be whole seconds' >&2; exit 2;; esac
case "$jobs" in ''|*[!0-9]*|0) echo 'jobs must be a positive integer' >&2; exit 2;; esac
case "$crate" in *[!A-Za-z0-9_-]*) echo 'bad crate name' >&2; exit 2;; esac
for f in "${files[@]}"; do [ -f "$ROOT/$f" ] || { echo "source file not found (repo-relative path expected): $f" >&2; exit 2; }; done

# Own target dir OUTSIDE the worktree (cargo-mutants copies the source tree; a shared target dir gives false MISSED).
export CARGO_TARGET_DIR="$TMPBASE/mut-target-$crate" CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
# Golden files live at the repo root (tests/golden), but cargo-mutants builds in a temp COPY and a reused target dir keeps pr-test-support
# "Fresh" from an earlier copy (compile-time CARGO_MANIFEST_DIR then points at a deleted dir: "no tests/golden above ...", every golden test
# fails and its mutants count as caught). pr_test_support::golden now resolves the root at run time; this variable pins it explicitly.
export PHOTOREVIEW_GOLDEN_ROOT="$ROOT"
# A reused target dir can also hold artifacts built from the LAST mutant of an earlier run; the fresh copy keeps older mtimes, cargo calls
# them Fresh and the unmutated baseline fails (or a survivor is judged against a mutated build). Bump the mtime of the crate sources.
find "$ROOT/crates/$crate/src" "$ROOT/crates/$crate/tests" -name '*.rs' -exec touch {} + 2>/dev/null
mkdir -p "$TMPBASE" || exit 1
out=${MUT_OUT:-$(mktemp -d "$TMPBASE/mut-out-$crate-XXXXXX")}
mkdir -p "$out" || exit 1
echo "crate: $crate  files: ${files[*]}  budget: ${budget}s  jobs: $jobs"
echo "target dir: $CARGO_TARGET_DIR"; echo "output dir: $out"

# What `cargo test` receives (besides -p and the cargo-args): test binaries, the user's extra args, then the libtest part after `--`.
testargs=()
if [ -n "$tests" ]; then
  IFS=',' read -r -a tl <<< "$tests"
  for t in "${tl[@]}"; do [ -n "$t" ] && testargs+=(--test "$t"); done
fi
testargs+=("${usertc[@]}")
if [ "$native" = 1 ]; then testargs+=(-- --include-ignored --test-threads=1); fi
lint_libtest "${testargs[@]}" || exit 2

# Preflight (the fix for AUDIT-MUTATION-FLAG-2026-10-30): with --baseline skip cargo-mutants never checks that the test command works, and a
# command cargo rejects makes EVERY mutant "caught". Run the unmutated test command once, with exactly these arguments; it must exit 0 and
# execute at least one test. MUT_PREFLIGHT=0 skips it (selftests with a fake cargo only).
preflight() {
  local log="$out/preflight.log" rc n
  echo "preflight: $CARGO_BIN test -p $crate ${cargoargs[*]} ${testargs[*]}"
  ( cd "$ROOT" && "$TIMEOUT_BIN" "$budget" "$CARGO_BIN" test -p "$crate" "${cargoargs[@]}" "${testargs[@]}" ) > "$log" 2>&1
  rc=$?
  n=$(grep -E '^running [0-9]+ tests?' "$log" | awk '{s+=$2} END{print s+0}')
  if [ $rc -ne 0 ] || [ "$n" -le 0 ]; then
    echo "PREFLIGHT FAILED (exit $rc, $n tests executed): the unmutated test command does not run tests, so every mutant would be reported caught. Log: $log" >&2
    tail -n 15 "$log" >&2
    exit 5
  fi
  echo "preflight ok: $n tests executed"
}
[ "${MUT_PREFLIGHT:-1}" = 0 ] || preflight

run_once() { # outdir, extra args...
  local od="$1"; shift
  mkdir -p "$od"
  local fargs=() cma=(); for f in "${files[@]}"; do fargs+=(--file "$f"); done
  for f in "${cargoargs[@]}"; do cma+=("--cargo-arg=$f"); done
  for f in "${testargs[@]}"; do cma+=("--cargo-test-arg=$f"); done
  ( cd "$ROOT" && "$TIMEOUT_BIN" "$budget" "$CARGO_BIN" mutants -p "$crate" --baseline skip --timeout "${MUT_TIMEOUT:-90}" -j "$jobs" \
      --output "$od" "${fargs[@]}" "${mfilt[@]}" "${cma[@]}" "$@" )
  rc=$?
  if grep -rlE 'no tests/golden above|PHOTOREVIEW_GOLDEN_ROOT=.* has no tests/golden' "$od/mutants.out/log" >/dev/null 2>&1; then
    echo "WARNING: golden files were NOT found inside the cargo-mutants copy (see $od/mutants.out/log). Mutants caught only by golden tests are NOT really caught: rerun them." >&2
  fi
  if [ $rc -eq 124 ]; then echo "BUDGET EXHAUSTED after ${budget}s: the result below is PARTIAL, not a full mutation verdict"; fi
  return $rc
}
parse() { # outdir label
  MODE=table OUTD="$1" LABEL="$2" "$PY_BIN" -I - <<'PY'
import os,sys,json,re,pathlib
od=pathlib.Path(os.environ['OUTD']); mo=od/'mutants.out'; label=os.environ['LABEL']
def lines(p):
    try: return [l.strip() for l in p.read_text(encoding='utf-8',errors='replace').splitlines() if l.strip()]
    except Exception: return []
j=None
try: j=json.loads((mo/'outcomes.json').read_text(encoding='utf-8'))
except Exception: j=None
cnt={'caught':0,'missed':0,'unviable':0,'timeout':0}; missed=[]; total=None
if isinstance(j,dict):
    total=j.get('total_mutants')
    for o in j.get('outcomes',[]):
        sc=o.get('scenario'); s=o.get('summary')
        if not isinstance(sc,dict) or 'Mutant' not in sc: continue
        m=sc['Mutant']
        key={'CaughtMutant':'caught','MissedMutant':'missed','Unviable':'unviable','Timeout':'timeout'}.get(s)
        if not key: continue
        cnt[key]+=1
        if key=='missed':
            name=m.get('name') or ''; f=m.get('file') or ''
            if not re.match(r'^\S+:\d+:\d+: ',name):
                sp=(m.get('span') or {}).get('start') or {}
                if not name:
                    fn=(m.get('function') or {}).get('function_name','?'); name='replace %s with %s'%(fn,m.get('replacement','?'))
                name='%s:%s:%s: %s'%(f,sp.get('line','?'),sp.get('column','?'),name)
            missed.append(name)
else:
    txt=lines(mo/'missed.txt'); missed=txt
    cnt['missed']=len(txt); cnt['caught']=len(lines(mo/'caught.txt')); cnt['unviable']=len(lines(mo/'unviable.txt')); cnt['timeout']=len(lines(mo/'timeout.txt'))
    if not txt and not (mo/'caught.txt').exists():
        print('%s: no outcomes.json and no *.txt under %s (the run did not get far enough to record outcomes)'%(label,mo)); sys.exit(0)
tested=sum(cnt.values())
print('%s: mutation results'%label)
print('  %-10s %s'%('caught',cnt['caught'])); print('  %-10s %s'%('missed',cnt['missed']))
print('  %-10s %s'%('unviable',cnt['unviable'])); print('  %-10s %s'%('timeout',cnt['timeout']))
print('  tested %d / total %s'%(tested,total if total is not None else '?'))
if total is not None and tested<total: print('  NOT RUN: %d mutants (partial run)'%(total-tested))
name='survivors.txt' if label=='run' else 'survivors-rerun.txt'
with open(od/name,'w',encoding='utf-8',newline='\n') as f:
    for m in missed: f.write(m+'\n')
print('  %s: %d line(s) -> %s'%(name,len(missed),od/name))
# regex filters for a rerun (one per line)
def esc(s): return re.sub(r'([\\.+*?()|\[\]{}^$])',r'\\\1',s)
with open(od/'filters.txt','w',encoding='utf-8',newline='\n') as f:
    for m in missed: f.write(esc(m)+'\n')
PY
}

run_once "$out"; rc=$?
echo "cargo-mutants exit code: $rc (0 all caught, 2 missed, 3 timeouts, 4 baseline failed, 124 budget)"
parse "$out" run
if [ "$rerun" = 1 ]; then
  fl=(); if [ -f "$out/filters.txt" ]; then while IFS= read -r l; do l=${l%$'\r'}; [ -n "$l" ] && fl+=(-F "$l"); done < "$out/filters.txt"; fi
  if [ ${#fl[@]} -eq 0 ]; then echo "no survivors to rerun"
  else
    echo "rerunning ${#fl[@]} survivor filter(s)"
    run_once "$out/rerun" "${fl[@]}"; rc2=$?
    echo "cargo-mutants exit code: $rc2"
    parse "$out/rerun" rerun
  fi
fi
case $rc in 0|2|3) exit 0;; *) exit "$rc";; esac
