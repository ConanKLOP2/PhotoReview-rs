#!/usr/bin/env bash
# Time-boxed cargo-mutants run done the way Wave 8 taught us. Never kills processes by name.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CARGO_BIN=${CARGO_BIN:-cargo}; PY_BIN=${PY_BIN:-python}; TIMEOUT_BIN=${TIMEOUT_BIN:-timeout}
TMPBASE=${TEMP:-${TMPDIR:-/tmp}}
usage() { echo 'usage: mutants.sh <crate> <src-file>... [--tests a,b] [--budget SECONDS] [--jobs N] [--rerun-survivors]' >&2; exit 2; }
crate=""; files=(); tests=""; budget=1200; jobs=1; rerun=0
while [ $# -gt 0 ]; do
  case "$1" in
    --tests) [ $# -ge 2 ] || usage; tests="$2"; shift 2;;
    --budget) [ $# -ge 2 ] || usage; budget="$2"; shift 2;;
    --jobs) [ $# -ge 2 ] || usage; jobs="$2"; shift 2;;
    --rerun-survivors) rerun=1; shift;;
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
mkdir -p "$TMPBASE" || exit 1
out=${MUT_OUT:-$(mktemp -d "$TMPBASE/mut-out-$crate-XXXXXX")}
mkdir -p "$out" || exit 1
echo "crate: $crate  files: ${files[*]}  budget: ${budget}s  jobs: $jobs"
echo "target dir: $CARGO_TARGET_DIR"; echo "output dir: $out"

testargs=()
if [ -n "$tests" ]; then
  IFS=',' read -r -a tl <<< "$tests"
  for t in "${tl[@]}"; do [ -n "$t" ] && testargs+=(--test "$t"); done
fi

run_once() { # outdir, extra args...
  local od="$1"; shift
  mkdir -p "$od"
  local fargs=(); for f in "${files[@]}"; do fargs+=(--file "$f"); done
  ( cd "$ROOT" && "$TIMEOUT_BIN" "$budget" "$CARGO_BIN" mutants -p "$crate" --baseline skip --timeout 90 -j "$jobs" \
      --output "$od" "${fargs[@]}" "$@" -- "${testargs[@]}" )
  rc=$?
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
