#!/usr/bin/env bash
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GIT_BIN=${GIT_BIN:-git}; GH_BIN=${GH_BIN:-gh}; PY_BIN=${PY_BIN:-python}; PS_BIN=${PS_BIN:-powershell.exe}
USAGE_BIN=${USAGE_BIN:-$ROOT/tools/autopilot/codex-usage.sh}
mode=${1:-}; [ -z "$mode" ] || [ "$mode" = --json ] || { echo 'usage: status.sh [--json]' >&2; exit 2; }
export ROOT GIT_BIN GH_BIN PY_BIN PS_BIN USAGE_BIN
"$PY_BIN" - "$mode" <<'PY'
import os,sys,json,subprocess,pathlib
R=pathlib.Path(os.environ['ROOT'])
def run(args,timeout=8):
 try:
  if str(args[0]).endswith('.sh'):args=['bash']+args
  return subprocess.run(args,cwd=R,text=True,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=timeout).stdout.strip()
 except:return ''
def ps(s):
 try:
  a=[os.environ['PS_BIN'],'-NoProfile','-Command',s]
  if a[0].endswith('.sh'):a=['bash']+a
  return subprocess.run(a,text=True,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=12).stdout.strip()
 except:return ''
def usage():
 try:
  d=json.loads(run(['bash',os.environ['USAGE_BIN'],'95'],25)); return {'ok':d.get('ok',False),'five_hour_percent':d['five_hour_used'],'five_hour_reset':d['five_hour_resets_at'],'week_percent':d['weekly_used'],'week_reset':d['weekly_resets_at'],'status':'STOP' if float(d['five_hour_used'])>=95 or float(d['weekly_used'])>=95 else 'OK'}
 except:return 'n/a'
quota=usage(); drive=str(R)[0]; free=ps(f"$d=Get-PSDrive -Name '{drive}'; [math]::Round($d.Free/1GB,2)")
try: free=float(free) if free else None
except: free=None
proc={k:'n/a' for k in ['codex-pipeline.sh','codex-run.sh','cargo','rustc','cargo-mutants','codex.exe']}
procraw=ps("Get-CimInstance Win32_Process | Where-Object { $_.Name -ne 'powershell.exe' -or $_.CommandLine -notmatch 'tools[\\\\/]dev[\\\\/]status[.]sh' } | ForEach-Object { [pscustomobject]@{ Name=$_.Name; CommandLine=$_.CommandLine } } | ConvertTo-Json -Compress")
try:
 plist=json.loads(procraw) if procraw else None
 if isinstance(plist,dict):plist=[plist]
 if plist is not None:
  proc={k:0 for k in proc}
  for p in plist:
   name=str(p.get('Name','')).lower(); cmd=str(p.get('CommandLine') or '').lower()
   if name=='cargo.exe':proc['cargo']+=1
   elif name=='rustc.exe':proc['rustc']+=1
   elif name=='cargo-mutants.exe':proc['cargo-mutants']+=1
   elif name=='codex.exe':proc['codex.exe']+=1
   if name=='bash.exe':
    for k in ('codex-pipeline.sh','codex-run.sh'):
     if k in cmd:proc[k]+=1
except:pass
state=R/'tools/autopilot/state'; pipes=[]
for f in sorted(state.glob('*.json')):
 try:
  d=json.loads(f.read_text(encoding='utf-8')); units=d.get('units',d); counts={'done':0,'partial':0,'failed':0,'skipped':0,'running/other':0}; failed=[]
  for u,v in (units.items() if isinstance(units,dict) else []):
   s=str(v.get('status','') if isinstance(v,dict) else v).lower(); counts[s if s in ('done','partial','failed','skipped') else 'running/other']+=1
   if s=='failed':
    reason='n/a'; p=state/'needs-claude'/(u+'.md')
    try:reason=next(x.split(':',1)[1].strip() for x in p.read_text(encoding='utf-8').splitlines() if x.startswith('Reason:'))
    except:pass
    failed.append({'unit':u,'reason':reason})
  pipes.append({'tag':f.stem,'counts':counts,'failed':failed})
 except:pass
logs=sorted((state/'logs').glob('*.out'),key=lambda p:p.stat().st_mtime); newest=None
if logs:
 try:
  ls=[x for x in logs[-1].read_text(errors='replace').splitlines() if x.strip() and not x.lstrip().startswith(('│','|','+'))]; newest={'file':logs[-1].name,'last_lines':ls[-2:]}
 except:pass
machine=os.environ.get('PR_MACHINE','').strip()
if not machine:
 try:machine=(state/'machine').read_text().strip()
 except:pass
if not machine:machine=run(['hostname']) or 'n/a'
git_ok=bool(run([os.environ['GIT_BIN'],'--version']))
fetch_ok=True
if os.environ.get('NO_FETCH')!='1':
 try:
  a=[os.environ['GIT_BIN'],'fetch','-q','origin','+refs/claims/*:refs/claims/*']
  if a[0].endswith('.sh'):a=['bash']+a
  fetch_ok=subprocess.run(a,cwd=R,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=12).returncode==0
 except:fetch_ok=False
raw=run([os.environ['GIT_BIN'],'for-each-ref','--format=%(refname:strip=2)|%(authorname)','refs/claims/']) if git_ok and fetch_ok else ''
claims='n/a' if not git_ok or not fetch_ok else {'machine':machine,'counts_by_machine':{},'mine':[]}
if isinstance(claims,dict):
 for line in raw.splitlines():
  a=line.split('|',1); claims['counts_by_machine'][a[-1]]=claims['counts_by_machine'].get(a[-1],0)+1
  if len(a)>1 and a[1]==machine:claims['mine'].append(a[0])
prs='n/a'
try:
 op=json.loads(run([os.environ['GH_BIN'],'pr','list','--state','open','--json','number,mergeStateStatus,title'])); me=json.loads(run([os.environ['GH_BIN'],'pr','list','--state','merged','--json','number,title','--limit','5'])); prs={'open':[dict(number=x.get('number'),mergeStateStatus=x.get('mergeStateStatus'),title=x.get('title','')[:70]) for x in op],'last_5_merged':me}
except:pass
wt=[]
for b in run([os.environ['GIT_BIN'],'worktree','list','--porcelain']).split('\n\n'):
 p=next((x[9:] for x in b.splitlines() if x.startswith('worktree ')),None)
 if p:
  q=pathlib.Path(p); under='.claude/worktrees' in str(q).replace('\\','/'); dirty=bool(run([os.environ['GIT_BIN'],'-C',str(q),'status','--porcelain'])) if under else False; wt.append({'name':q.name,'under':under,'dirty':dirty,'target':(q/'target').is_dir()})
data={'quota':quota,'disk_free_gb':free,'processes':proc,'pipelines':pipes,'newest_log':newest,'claims':claims,'pull_requests':prs,'worktrees':{'count':len(wt),'dirty_tracked_under_claude_worktrees':sum(x['under'] and x['dirty'] for x in wt),'with_target':[x['name'] for x in wt if x['target']]} if git_ok else 'n/a'}
if sys.argv[1]=='--json':print(json.dumps(data));sys.exit(0)
if quota=='n/a':print('QUOTA: n/a')
else:print('QUOTA: 5h %s (reset %s), week %s (reset %s), ok=%s, %s'%(quota['five_hour_percent'],quota['five_hour_reset'],quota['week_percent'],quota['week_reset'],quota['ok'],quota['status']))
print('DISK: %s%s'%(str(free)+' GB' if free is not None else 'n/a',' LOW DISK' if free is not None and free<20 else ''))
print('PROCESSES: '+'; '.join('%s=%s'%(k,v) for k,v in proc.items()))
for p in pipes:
 print('PIPELINE %s: '%p['tag']+', '.join('%s=%s'%(k,v) for k,v in p['counts'].items()))
 for x in p['failed']:print('  FAILED %s: %s'%(x['unit'],x['reason']))
print('NEWEST LOG: '+(newest['file']+' | '+' | '.join(newest['last_lines']) if newest else 'n/a'))
if isinstance(claims,dict):
 print('CLAIMS BY MACHINE: '+(', '.join('%s=%s'%(k,v) for k,v in claims['counts_by_machine'].items()) or 'none'))
 print('CLAIMS HELD BY %s: '%machine+(', '.join(claims['mine']) or 'none'))
else:print('CLAIMS: n/a')
if isinstance(prs,dict):
 print('PULL REQUESTS OPEN: '+('; '.join('#%s [%s] %s'%(x['number'],x['mergeStateStatus'],x['title']) for x in prs['open']) or 'none'))
 print('PULL REQUESTS MERGED: '+('; '.join('#%s %s'%(x.get('number'),x.get('title','')[:70]) for x in prs['last_5_merged']) or 'none'))
else:print('PULL REQUESTS: n/a')
if isinstance(data['worktrees'],dict):print('WORKTREES: count=%s dirty-tracked=%s with-target=%s'%(data['worktrees']['count'],data['worktrees']['dirty_tracked_under_claude_worktrees'],','.join(data['worktrees']['with_target']) or 'none'))
else:print('WORKTREES: n/a')
PY
