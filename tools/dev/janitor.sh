#!/usr/bin/env bash
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GIT_BIN=${GIT_BIN:-git}; GH_BIN=${GH_BIN:-gh}; PY_BIN=${PY_BIN:-python}; PS_BIN=${PS_BIN:-powershell.exe}; DU_BIN=${DU_BIN:-du}; DU_TIMEOUT=${DU_TIMEOUT:-20}; CMD_BIN=${CMD_BIN:-cmd.exe}
apply=0; claims=0
for x in "$@"; do case "$x" in --apply) apply=1;; --claims) claims=1;; *) echo 'usage: janitor.sh [--apply] [--claims]' >&2; exit 2;; esac; done
export ROOT GIT_BIN GH_BIN PY_BIN PS_BIN DU_BIN DU_TIMEOUT CMD_BIN APPLY=$apply CLAIMS=$claims
"$PY_BIN" - <<'PY'
import os,subprocess,pathlib,json,time,shutil,re,sys
R=pathlib.Path(os.environ['ROOT']); G=os.environ['GIT_BIN']; H=os.environ['GH_BIN']; PS=os.environ['PS_BIN']; applying=os.environ['APPLY']=='1'
failures=[]; removed=[]
def pathobj(s):
 if os.name=='nt' and re.match(r'^/[A-Za-z]/',str(s)):s=str(s)[1].upper()+':/'+str(s)[3:]
 return pathlib.Path(s)
def pathkey(s):
 x=str(s).replace('\\','/')
 if re.match(r'^/[A-Za-z]/',x):x=x[1].upper()+':'+x[2:]
 return os.path.normcase(x).replace('\\','/').rstrip('/').lower()
def run(a,timeout=15):
 try:
  if str(a[0]).endswith('.sh'):a=['bash']+a
  return subprocess.run(a,cwd=R,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=timeout)
 except Exception as e:return None
def out(a,timeout=15):
 p=run(a,timeout); return p.stdout.strip() if p and p.returncode==0 else ''
def git(*a):return out([G,*a])
def powershell(s):
 p=run([PS,'-NoProfile','-Command',s],12); return p.stdout.strip() if p and p.returncode==0 else None
def disk():
 v=powershell("$d=Get-PSDrive -Name '%s'; [math]::Round($d.Free/1GB,2)"%str(R)[0])
 try:return float(v) if v else None
 except:return None
def size(p):
 q=run([os.environ['DU_BIN'],'-sk',str(p)],float(os.environ['DU_TIMEOUT']))
 if q is None:return '~'
 try:return int(q.stdout.split()[0])*1024 if q.returncode==0 else '~'
 except:return '~'
def clean(p):
 s=git('-C',str(p),'status','--porcelain','--untracked-files=all')
 if not s:return True
 return all(x.startswith('?? ') and any(pathlib.PurePosixPath(x[3:].replace('\\','/')).parts[0].startswith(z) or x[3:].lower().endswith(z) for z in ('.codex','target','mutants.out','.log')) for x in s.splitlines())
def merged(br):
 if not br:return False
 try:
  p=run([H,'pr','list','--head',br,'--state','merged','--json','number']);
  if p and p.returncode==0 and json.loads(p.stdout):return True
 except:pass
 q=run([G,'merge-base','--is-ancestor',br,'origin/master']); return q is not None and q.returncode==0
def process_lines():
 v=powershell("Get-CimInstance Win32_Process | ForEach-Object { $_.CommandLine }")
 if v is None:return None
 return v.splitlines()
procs=process_lines(); process_unreadable=procs is None
def in_use(p,branch=''):
 if (R/'tools/autopilot/state/pipeline.lock').exists() and (branch.startswith('integration/') or branch.startswith('wu-') or branch.startswith('wu/') or pathlib.Path(p).name.startswith('wu-')):return True
 if process_unreadable:return True
 variants={str(p).replace('/','\\').lower(),str(p).replace('\\','/').lower()}
 return any(any(v in line.lower() for v in variants) for line in procs)
def path_age(p):
 try:return time.time()-p.stat().st_mtime
 except:return 0
worktrees=[]
for b in git('worktree','list','--porcelain').split('\n\n'):
 p=next((x[9:] for x in b.splitlines() if x.startswith('worktree ')),None); br=next((x[7:] for x in b.splitlines() if x.startswith('branch ')),None); locked=[x[7:] for x in b.splitlines() if x.startswith('locked ')]
 if p:worktrees.append((pathobj(p),br.replace('refs/heads/','') if br else '',locked[0] if locked else ''))
candidates=[]; removed_wt=set(); branch_by_path={};
for p,br,lockreason in worktrees:
 if pathkey(p)==pathkey(R) or '.claude/worktrees' not in str(p).replace('\\','/').lower():continue
 if lockreason:
  if 'initializing' in lockreason.lower() and path_age(p)>3600 and not in_use(p,br) and clean(p):
   candidates.append(('worktree-locked',str(p),'initializing lock older than 1 hour',size(p)));removed_wt.add(str(p));branch_by_path[str(p)]=br
  continue
 if in_use(p,br):continue
 try:
  q=run([H,'pr','list','--head',br,'--state','open','--json','number']); openpr=bool(json.loads(q.stdout)) if q and q.returncode==0 else None
 except:openpr=None
 if br.startswith('integration/') and (R/'tools/autopilot/state/pipeline.lock').exists():continue
 if openpr is not False:continue
 if merged(br) and clean(p):candidates.append(('worktree',str(p),'clean; branch merged',size(p)));removed_wt.add(str(p));branch_by_path[str(p)]=br
for p,br,lockreason in worktrees:
 if pathkey(p)==pathkey(R) or str(p) in removed_wt or in_use(p,br) or not clean(p):continue
 for n in ('target','target-mut'):
  q=p/n
  if q.is_dir() and not in_use(q,br):candidates.append(('target',str(q),'rebuildable; worktree kept',size(q)))
base=R/'.claude/worktrees'
if base.exists():
 registered={pathkey(p) for p,_,_ in worktrees}
 for p in base.iterdir():
  if pathkey(p) not in registered and p.is_dir():
   files=[x for x in p.rglob('*') if x.is_file()]; safe=all(x.name.startswith(('.codex','target','mutants.out')) or x.suffix=='.log' for x in files)
   if safe and not in_use(p):candidates.append(('orphan',str(p),'unregistered disposable directory',size(p)))
  elif p.is_file() and p.suffix.lower() in ('.jsonl','.err','.txt','.log'):candidates.append(('loose-file',str(p),'loose worktree file',size(p)))
state=R/'tools/autopilot/state'
for ref in git('for-each-ref','--format=%(refname:strip=2)','refs/pipeline-old/').splitlines():
 unit=ref.split('/')[-1]
 for f in state.glob('*.json'):
  try:
   v=json.loads(f.read_text(encoding='utf-8')).get('units',{}).get(unit,{}); s=v.get('status','') if isinstance(v,dict) else v
   if s in ('done','partial'):candidates.append(('old-ref',ref,'unit '+s,0));break
  except:pass
temp=os.environ.get('TEMP')
if temp:
 prot=[]
 for q in (R/'task_on_progress.md',R/'docs'):
  if q.exists():prot.extend(x.read_text(errors='ignore') for x in ([q] if q.is_file() else q.rglob('*')) if x.is_file())
 protected='\n'.join(prot)
 try:
  for p in pathlib.Path(temp).iterdir():
   if p.is_dir() and not p.name.startswith('codex-') and (p.name.startswith('mut-') or p.name.endswith('-mut') or p.name.startswith('mut-out')) and p.name not in protected and path_age(p)>86400:candidates.append(('temp',str(p),'mutation scratch older than 1 day',size(p)))
 except:pass
machine=os.environ.get('PR_MACHINE','').strip()
if not machine:
 try:machine=(state/'machine').read_text().strip()
 except:pass
if not machine:
 try:machine=subprocess.check_output(['hostname'],text=True).strip()
 except:machine=''
def merged_units():
 script=R/'tools/autopilot/lib/codex-lib.py'; units=set()
 try:
  for line in out([G,'log','origin/master','--format=%s']).splitlines():
   if line.startswith(('docs(','docs:','chore(','chore:','ci(','style(')):continue
   code="import runpy,sys,re; f=runpy.run_path(sys.argv[1]); t=sys.stdin.read(); ids=f['expand_unit_ids'](t); waves=list(re.finditer(r'\\b(W\\d+)-\\d\\d[a-z]',t));\nfor i,m in enumerate(waves):\n end=waves[i+1].start() if i+1<len(waves) else len(t); tail=t[m.end():end];\n for group in re.findall(r'(?<![\\w-])(\\d\\d[a-z](?:\\s*,\\s*\\d\\d[a-z])*)',tail): ids += [m.group(1)+'-'+x.strip() for x in re.split(r'\\s*,\\s*',group)];\nprint('\\n'.join(dict.fromkeys(ids)))"
   q=subprocess.run([os.environ['PY_BIN'],'-c',code,str(script)],cwd=R,input=line,text=True,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=8)
   if q.returncode==0:units.update(x.strip() for x in q.stdout.splitlines() if x.strip())
 except:pass
 return units
if os.environ.get('CLAIMS')=='1':
 run([G,'fetch','-q','origin','+refs/claims/*:refs/claims/*'])
 units=merged_units()
 for unit in git('for-each-ref','--format=%(refname:strip=2)','refs/claims/').splitlines():
  author=git('log','-1','--format=%an','refs/claims/'+unit)
  if unit in units and author==machine:candidates.append(('claim',unit,'owned by this machine; unit on origin/master',0))
print('KIND\tPATH\tREASON\tSIZE_BYTES')
for k,p,reason,n in candidates:print(f'{k}\t{p}\t{reason}\t{n}')
print('TOTAL\t'+str(len(candidates)))
total_size=sum(n for _,_,_,n in candidates if isinstance(n,int)); print('TOTAL SIZE_BYTES\t'+str(total_size)+(' (approximate paths present)' if any(not isinstance(n,int) for _,_,_,n in candidates) else ''))
if process_unreadable:print('IN-USE: process list unavailable; worktree and target removal candidates suppressed')
before=disk() if applying else None
if applying:
 print('DISK BEFORE:',before if before is not None else 'n/a','GB')
 freed=0; approximate=False
 for k,p,reason,n in candidates:
  try:
   if k in ('worktree','worktree-locked'):
    if k=='worktree-locked':
     z=run([G,'worktree','unlock',p])
     if z is None or z.returncode:failures.append(p+' (unlock failed)');continue
    z=run([G,'worktree','remove','--force',p])
    if (z is None or z.returncode!=0 or pathlib.Path(p).exists()):
     run([G,'worktree','prune'])
     win=str(pathlib.Path(p).resolve()).replace('/','\\'); z=run([os.environ['CMD_BIN'],'/c','rmdir','/s','/q','\\\\?\\'+win])
    if pathlib.Path(p).exists():failures.append(p+' (worktree removal failed)');continue
    br=branch_by_path.get(p,'')
    if br and merged(br):
     z=run([G,'branch','-D',br])
     if z is None or z.returncode:failures.append(br+' (branch delete failed)')
   elif k=='target':shutil.rmtree(p)
   elif k in ('orphan','temp'):shutil.rmtree(p)
   elif k=='loose-file':pathlib.Path(p).unlink()
   elif k=='old-ref':run([G,'update-ref','-d','refs/pipeline-old/'+p])
   elif k=='claim':
    z=subprocess.run(['bash',str(R/'tools/autopilot/claim.sh'),'release',p],cwd=R,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    if z.returncode:failures.append(p+' (claim release failed: '+z.stderr.strip()+')');continue
   removed.append(f'{k}:{p}')
   if isinstance(n,int):freed+=n
   else:approximate=True
  except Exception as e:failures.append(p+' ('+str(e)+')')
 after=disk(); print('REMOVED:',','.join(removed) if removed else 'none'); print('DISK AFTER:',after if after is not None else 'n/a','GB')
 print('FREED SIZE:',('~' if approximate else '')+str(freed),'bytes')
if failures:
 for x in failures:print('FAILED:',x)
PY
