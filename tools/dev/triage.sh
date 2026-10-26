#!/usr/bin/env bash
# Classify a failed autopilot unit and suggest the next action. Read-only; never starts an agent.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PY_BIN=${PY_BIN:-python}
STATE_DIR=${STATE_DIR:-$ROOT/tools/autopilot/state}
CONTRACTS_DIR=${CONTRACTS_DIR:-$ROOT/contracts}
unit=""; prompt=""; all=0
usage() { echo 'usage: triage.sh <unit>|--all [--prompt FILE]' >&2; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --prompt) [ $# -ge 2 ] || usage; prompt="$2"; shift 2;;
    --all) all=1; shift;;
    -*) usage;;
    *) unit="$1"; shift;;
  esac
done
[ -n "$unit" ] || [ "$all" = 1 ] || usage
export STATE_DIR CONTRACTS_DIR UNIT="$unit" ALL=$all PROMPT_FILE="$prompt"
"$PY_BIN" -I - <<'PY'
import os,re,sys,glob,pathlib
S=pathlib.Path(os.environ['STATE_DIR']); C=pathlib.Path(os.environ['CONTRACTS_DIR'])
ANSI=re.compile(r'\x1b\[[0-9;]*[A-Za-z]|\[(?:\d+;?)*m')
def rd(p):
    try: return ANSI.sub('',pathlib.Path(p).read_text(encoding='utf-8',errors='replace')).replace('\r\n','\n')
    except Exception: return ''
CLASSES=[
 ('environment',r'STATUS_DLL_INIT_FAILED|os error (?:5|112|206)\b|Access is denied|No space left|not enough space on the disk|There is not enough space',
  'Environment problem, not a code problem: free disk/RAM (tools/dev/janitor.sh), make sure no other agent holds the target dir, then rerun the gate once. Do not edit code.'),
 ('compile-error',r'error\[E\d{4}\]|could not compile',
  'Fix the compile errors in the owned files (read the first error[E....] and its span; later ones are often cascades). Run cargo check -p <crate> --tests.'),
 ('red-test-guard',r'weakened',
  'The guard saw a test modified. Restore the test from HEAD; correct a test ONLY if the C# source contradicts it (cite file and line), otherwise fix the implementation.'),
 ('stubs-left',r'\btodo!\s*\(|\bunimplemented!\s*\(|not yet implemented|not implemented',
  'Finish the stubs: replace every todo!()/unimplemented!() left in the owned source with the real port of the C# code.'),
 ('test-failure',r'panicked at|assertion (?:`left == right` )?failed|^\s*left:|^\s*right:|test result: FAILED',
  'Read the failing assertion and compare with the C# source. Fix the implementation; correct the test only if the C# source contradicts it. Rerun alone the known flaky tests (pr-app misc_app_log, pr-imaging cache_preview_file) before changing code.'),
 ('rustfmt-only',r'^Diff in ',
  'Formatting only: run cargo fmt -p <crate> (CI formats per package), review the diff, commit. No logic change needed.'),
 ('mutants-budget',r'(?i)mutant.*(?:budget|survivor|missed|timed? ?out)|MISSED\s|budget exceeded',
  'Mutation stage ran out of budget or left survivors: run tools/dev/mutants.sh on the risky functions only, kill survivors with tests or explain them as equivalent.'),
]
def section(txt,name):
    m=re.search(r'^## '+re.escape(name)+r'\n(.*?)(?=^## |\Z)',txt,re.S|re.M)
    if not m: return ''
    b=m.group(1).strip('\n')
    b=re.sub(r'^```[a-z]*\n','',b); b=re.sub(r'\n?```\s*$','',b)
    return b
def classify(unit):
    note=rd(S/'needs-claude'/(unit+'.md'))
    logs=S/'logs'
    gate=rd(logs/(unit+'.gate-error.txt'))
    logtxt=''.join(rd(p) for p in sorted(glob.glob(str(logs/(unit+'*.log')))))
    reason=(re.search(r'^Reason:\s*(.*)$',note,re.M) or [None,''])[1]
    tail=section(note,'Error tail'); failing=section(note,'Failing tests')
    found=[k for k,v in (('needs-claude note',note),('gate-error.txt',gate),('logs/*.log',logtxt)) if v.strip()]
    # classify on the most specific evidence first (note tail + gate error); logs only as a fallback
    primary='\n'.join([tail,gate,reason]); fallback=logtxt
    res=('unknown',[],'No known pattern. Read the tail by hand; if it is empty, the pipeline may have failed before cargo ran (see Reason).')
    for src in (primary,fallback):
        if not src.strip(): continue
        hit=False
        for name,rx,act in CLASSES:
            lines=[l for l in src.split('\n') if re.search(rx,l)]
            if lines:
                res=(name,lines,act); hit=True; break
        if hit: break
    name,ev,act=res
    flaky=[t for t in ('misc_app_log','cache_preview_file') if t in (tail+gate+failing)]
    return dict(unit=unit,cls=name,evidence=ev,action=act,reason=reason,tail=tail or gate,failing=failing,found=found,flaky=flaky)
def owned(unit):
    txt=rd(C/(unit+'.md'))
    m=re.search(r'^#+\s*Owned files[^\n]*\n(.*?)(?=^#+ |\Z)',txt,re.S|re.M)
    if not m: return None
    body=m.group(1); fm=re.search(r'```[a-z]*\n(.*?)```',body,re.S)
    block=fm.group(1) if fm else body
    out=[]
    for line in block.split('\n'):
        line=re.sub(r'^\s*[-*]\s*','',line)
        for tok in re.findall(r'`?([\w][\w./\\-]*\.\w+)`?',line.split('(')[0]):
            if '/' in tok and tok not in out: out.append(tok)
    return out
def show(r):
    print('unit: %s'%r['unit']); print('class: %s'%r['cls'])
    if r['reason']: print('reason: %s'%r['reason'])
    print('sources: %s'%(', '.join(r['found']) or 'none'))
    ev=r['evidence'][:12]
    print('evidence:')
    for l in ev: print('  '+l.strip()[:200])
    if not ev: print('  (none)')
    print('next: '+r['action'])
    if r['flaky']: print('note: failing tests mention known flaky %s: rerun them alone first'%', '.join(r['flaky']))
def make_prompt(r,path):
    own=owned(r['unit'])
    tl=r['tail'].split('\n')[-60:]
    p=['# Task: repair unit %s (failure class: %s)'%(r['unit'],r['cls']),'',
       'The gate for this unit failed. %s'%r['action'],'',
       '## Owned files (the ONLY files you may create or modify)']
    p+=['- '+f for f in own] if own else ['(contracts/%s.md has no Owned files list: read the contract and stay inside it)'%r['unit']]
    p+=['','## Error tail (last %d lines)'%len(tl),'```']+tl+['```','','## Rules',
        '- Never weaken, delete or loosen a test.',
        '- Correct a test only when the C# source contradicts it, and cite the C# file and line in your message.',
        '- Files are UTF-8, LF line endings, no BOM.',
        '- Touch nothing outside the owned files; do not start other agents or kill processes.','']
    pathlib.Path(path).write_text('\n'.join(p),encoding='utf-8',newline='\n')
    print('prompt written: %s'%path)
if os.environ['ALL']=='1':
    d=S/'needs-claude'
    units=sorted(p.stem for p in d.glob('*.md')) if d.is_dir() else []
    if not units: print('no needs-claude notes in %s (clean checkout or nothing failed)'%d); sys.exit(0)
    for u in units:
        r=classify(u); print('%-10s %-16s %s'%(u,r['cls'],r['reason']))
    sys.exit(0)
u=os.environ['UNIT']
if not re.match(r'^[A-Za-z0-9._-]+$',u): print('bad unit id',file=sys.stderr); sys.exit(2)
r=classify(u)
if not r['found']:
    print('unit: %s\nclass: unknown\nnothing to triage: no needs-claude note and no logs under %s (clean checkout?)'%(u,S)); sys.exit(0)
show(r)
if os.environ['PROMPT_FILE']: make_prompt(r,os.environ['PROMPT_FILE'])
PY
