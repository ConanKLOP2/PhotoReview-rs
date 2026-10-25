#!/usr/bin/env bash
set -u
PY="${PY_BIN:-python}"
input=$(cat)
repo=$(cd "$(dirname "$0")/../../.." && pwd)
"$PY" - "$input" "$repo" <<'PY'
import json, os, posixpath, re, sys

def tokenize(script):
    # Track quote state and operator identity explicitly, including empty words.
    tokens, word, active, quote = [], '', False, None
    i = 0
    def flush():
        nonlocal word, active
        if active: tokens.append((word, False))
        word, active = '', False
    while i < len(script):
        c = script[i]
        if quote:
            if c == quote: quote = None
            elif c == '\\' and quote == '"' and i+1 < len(script) and script[i+1] in '$`"\\\n':
                i += 1
                if script[i] != '\n': word += script[i]
            else: word += c
        elif c in "'\"": quote, active = c, True
        elif c == '\\':
            i += 1
            if i == len(script): raise ValueError('unfinished escape')
            if script[i] != '\n': word += script[i]; active = True
        elif c == '#' and not active:
            end = script.find('\n', i)
            i = len(script) if end < 0 else end
            continue
        elif c.isspace() or c in ';&|<>()':
            flush()
            if c == '\n': tokens.append((c, True))
            elif not c.isspace():
                op = c
                if i+1 < len(script) and script[i+1] == c and c in '&|<>':
                    i += 1; op += c
                tokens.append((op, True))
        else: word += c; active = True
        i += 1
    if quote: raise ValueError('unfinished quote')
    flush()
    return tokens

def commands(script):
    # Strip documents before tokenizing the complete script. Scan each header
    # with the same quote-aware lexer, then consume its pending documents.
    lines = script.splitlines(keepends=True)
    kept, i = [], 0
    while i < len(lines):
        header = lines[i]
        i += 1
        # A header may span quoted newlines.
        while True:
            try: ts = tokenize(header); break
            except ValueError:
                if i == len(lines): raise
                header += lines[i]; i += 1
        docs = []
        for n, (t, op) in enumerate(ts):
            if op and t == '<<':
                delimiter = ts[n+1][0]
                tabs = delimiter.startswith('-')
                docs.append((delimiter[1:] if tabs else delimiter, tabs))
        kept.append(header)
        for delimiter, tabs in docs:
            while i < len(lines):
                line = lines[i].rstrip('\r\n'); i += 1
                if (line.lstrip('\t') if tabs else line) == delimiter: break
            else: raise ValueError('unfinished heredoc')
    result, argv, redirect = [], [], False
    for t, op in tokenize(''.join(kept)):
        if op and any(c in t for c in ';&|()\n'):
            if argv: result.append(argv)
            argv, redirect = [], False
        elif op and ('<' in t or '>' in t): redirect = True
        elif redirect: redirect = False
        else: argv.append(t)
    if argv: result.append(argv)
    return result

def norm(p):
    p = p.replace('\\', '/')
    p = re.sub(r'^\$(?:HOME|\{HOME\})(?=/|$)', os.path.expanduser('~').replace('\\','/'), p)
    p = os.path.expanduser(p).replace('\\','/')
    p = re.sub(r'^([a-zA-Z]):/', lambda m: '/'+m[1].lower()+'/', p)
    if not p.startswith('/'):
        cwd = os.getcwd().replace('\\', '/')
        cwd = re.sub(r'^([a-zA-Z]):/', lambda m: '/'+m[1].lower()+'/', cwd)
        p = posixpath.join(cwd, p)
    return posixpath.normpath(p).lower()

def protected(ref):
    ref = ref.lstrip('+').rsplit(':',1)[-1].removeprefix('refs/heads/')
    return ref in ('master','main') or ref.startswith('integration/')

def inspect(script, depth=0):
    messages = []
    for a in commands(script):
        while a:
            if re.match(r'^[A-Za-z_][A-Za-z0-9_]*=',a[0]): a=a[1:]; continue
            if a[0].lower() not in ('command','env','sudo','time','nohup'): break
            wrapper, a=a[0].lower(), a[1:]
            while a and a[0].startswith('-'):
                option,a=a[0],a[1:]
                values = {'env': ('-u','--unset','-C','--chdir'),
                          'sudo': ('-u','-g','-h','-p','-C','-r','-t'),
                          'time': ('-f','--format','-o','--output')}
                if option in values.get(wrapper, ()) and a: a=a[1:]
        if not a: continue
        prog,args=a[0].lower(),a[1:]
        own=args[:args.index('--')] if '--' in args else args
        if prog=='cargo' and own[:1]==['fmt'] and not any(x in ('-p','--package') or x.startswith('--package=') or x.startswith('-p') for x in own[1:]):
            messages.append('Use cargo fmt -p <package> on Windows to avoid the os error 206.')
        if prog in ('taskkill','taskkill.exe') and any(x.lower() in ('/im','//im') and y.lower() in ('rustc.exe','cargo.exe','cargo-mutants.exe','link.exe','cl.exe','codex.exe') for x,y in zip(args,args[1:])):
            messages.append('Kill only the exact process ID with taskkill /PID <pid>.')
        if prog=='git' and args[:1]==['push']:
            force=any(x in ('--force','-f','--force-with-lease') or x.startswith('--force-with-lease=') for x in own)
            if any(protected(x) and (force or x.startswith('+')) for x in args[1:] if not x.startswith('-')):
                messages.append('Do not force-push protected branches; merge master and push normally.')
        if prog in ('rm','remove-item','ri'):
            flags=''.join(x[1:] for x in own if x.startswith('-') and not x.startswith('--')).lower()
            recursive=('r' in flags or '--recursive' in own) if prog=='rm' else any(x.lower()=='-recurse' for x in own)
            force='f' in flags or '--force' in own or prog!='rm'
            roots={norm(x) for x in (sys.argv[2],'.git','.claude','.claude/worktrees',sys.argv[2]+'/.git',sys.argv[2]+'/.claude',sys.argv[2]+'/.claude/worktrees','C:/MyProjects','/c/MyProjects','~')}
            if recursive and force and any(norm(x) in roots for x in args if not x.startswith('-')):
                messages.append('Remove only a specific temporary child path, never a repository or shared workspace root.')
        if prog=='git' and args[:1]==['clean']:
            paths=args[args.index('--')+1:] if '--' in args else [x for x in args[1:] if not x.startswith('-')]
            if not paths and any(x.startswith('-') and any(c in x[1:] for c in 'fdx') for x in own[1:]):
                messages.append('Pass an explicit path with git clean -fd -- <path>.')
        if depth==0 and prog in ('bash','sh','cmd','cmd.exe'):
            for n,x in enumerate(args[:-1]):
                if x.lower()==('/c' if prog.startswith('cmd') else '-c'):
                    messages.extend(inspect(args[n+1],1)); break
    return messages
try:
    messages=inspect(json.loads(sys.argv[1]).get('tool_input',{}).get('command',''))
except Exception:
    sys.exit(0)  # Fail open on any parser error.
if messages:
    print(messages[0],file=sys.stderr)
    sys.exit(2)
PY
