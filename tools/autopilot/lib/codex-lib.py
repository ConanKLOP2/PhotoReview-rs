#!/usr/bin/env python3
"""Helpers for the Claude-free Codex pipeline (plan/contract parsing, JSON state, gates, telemetry). No third-party imports."""
import json, os, re, subprocess, sys, time
sys.stdout.reconfigure(encoding="utf-8", newline=chr(10))

def read(p):
    with open(p, encoding="utf-8", errors="replace") as f:
        return f.read()

def plan_rows(plan):
    """Yield dicts for table rows `| W8-04a | files | rust | col | model | test | deps | batch |`."""
    for line in read(plan).splitlines():
        if not re.match(r"^\|\s*W\d+-\d\d[a-z]\s*\|", line):
            continue
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(c) < 8:
            continue
        yield {"id": c[0], "deps_raw": c[6], "batch": c[7], "line": line}

def expand_deps(raw, uid):
    prefix = uid.rsplit("-", 1)[0]
    raw = re.sub(r"`[^`]*`", " ", raw)
    out = []
    for a, b in re.findall(r"\b(\d\d[a-z])\.\.(\d\d[a-z])\b", raw):
        if a[:2] == b[:2]:
            out += [a[:2] + chr(x) for x in range(ord(a[2]), ord(b[2]) + 1)]
    raw = re.sub(r"\b\d\d[a-z]\.\.\d\d[a-z]\b", " ", raw)
    out += re.findall(r"\b(\d\d[a-z])\b", raw)
    out += re.findall(r"\bW\d+-(\d\d[a-z])\b", raw)
    seen, res = set(), []
    for t in out:
        d = prefix + "-" + t
        if d not in seen:
            seen.add(d); res.append(d)
    return res

def cmd_order(plan, batch, only=None):
    rows = [r for r in plan_rows(plan) if r["batch"].upper() == batch.upper()]
    ids = [r["id"] for r in rows]
    deps = {r["id"]: [d for d in expand_deps(r["deps_raw"], r["id"]) if d in ids and d != r["id"]] for r in rows}
    done, order = set(), []
    while len(order) < len(ids):
        prog = False
        for i in ids:
            if i not in done and all(d in done for d in deps[i]):
                done.add(i); order.append(i); prog = True
        if not prog:
            sys.exit("dependency cycle in batch: " + ",".join(i for i in ids if i not in done))
    sel = set(only.split(",")) if only else None
    for i in order:
        if sel is None or i in sel:
            print(i + "\t" + ",".join(deps[i]))

def cmd_row(plan, uid):
    for r in plan_rows(plan):
        if r["id"] == uid:
            print(r["line"]); return
    sys.exit("no plan row for " + uid)

def cmd_deps(plan, uid):
    for r in plan_rows(plan):
        if r["id"] == uid:
            print("\n".join(expand_deps(r["deps_raw"], uid))); return

def owned(contract):
    res, on = [], False
    for line in read(contract).splitlines():
        if not on:
            if re.search(r"Owned files", line) and re.search(r"only", line, re.I):
                on = True
            continue
        m = re.match(r"^\s{2,}`?([\w./-]+\.\w+)`?(\s|$)", line)
        if m and "/" in m.group(1):
            res.append(m.group(1)); continue
        if line.strip() == "":
            continue
        if re.match(r"^\s*(-\s*)?(Lead-owned|Read-only)|^#|^-\s", line) or re.match(r"^\S", line):
            break
        # continuation / comment line: keep scanning
    return res

def cmd_cs(contract):
    on, res = False, []
    for line in read(contract).splitlines():
        if line.startswith("## "):
            on = line.startswith("## C# ")
            continue
        if not on:
            continue
        for t in re.findall(r"`([^`\s]+\.cs)`", line):
            if "tools/" in t or "/Commands/" in t or re.match(r"W\d+-", t) or t in res:
                continue
            res.append(t)
    print("\n".join(res))

def cmd_baseline(contract):
    m = re.search(r"[Bb]aseline[^`\n]*`([0-9a-f]{7,40})`", read(contract))
    print(m.group(1) if m else "")

# ------------------------------------------------------------------ state (JSON, safe for parallel units)
def state_load(p):
    try:
        return json.loads(read(p))
    except Exception:
        return {"units": {}, "meta": {}}

class state_lock:
    """Cross-process lock for state read-modify-write (parallel units share the state file)."""
    def __init__(self, p):
        self.p = p + ".lock"
    def __enter__(self):
        os.makedirs(os.path.dirname(self.p) or ".", exist_ok=True)
        t0 = time.time()
        while True:
            try:
                os.close(os.open(self.p, os.O_CREAT | os.O_EXCL | os.O_WRONLY)); return self
            except FileExistsError:
                try:
                    if time.time() - os.path.getmtime(self.p) > 30: os.remove(self.p)
                except OSError:
                    pass
                if time.time() - t0 > 40: raise SystemExit("state lock timeout")
                time.sleep(0.05)
    def __exit__(self, *a):
        try: os.remove(self.p)
        except OSError: pass

def state_save(p, s):
    os.makedirs(os.path.dirname(p) or ".", exist_ok=True)
    tmp = p + ".tmp.%d" % os.getpid()
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(s, f, indent=1, sort_keys=True)
    os.replace(tmp, p)

def cmd_sset(path, uid, status, note="", surv=""):
    with state_lock(path):
        s = state_load(path); u = s["units"].setdefault(uid, {})
        u["status"] = status; u["note"] = note
        u["survivors"] = int(surv) if surv else u.get("survivors", 0)
        state_save(path, s)

def cmd_supd(path, uid, kvs):
    """supd file id key=value key+=number ... (atomic update of telemetry fields)."""
    with state_lock(path):
        s = state_load(path); u = s["units"].setdefault(uid, {})
        for kv in kvs:
            if "+=" in kv:
                k, v = kv.split("+=", 1)
                u[k] = round(u.get(k, 0) + float(v), 2) if "." in v else u.get(k, 0) + int(float(v))
            else:
                k, v = kv.split("=", 1)
                try: u[k] = int(v)
                except ValueError: u[k] = v
        state_save(path, s)

def cmd_avgpct(path):
    """Average 5h-quota percent spent per merged unit (empty when unknown)."""
    v = [u["pct"] for u in state_load(path)["units"].values() if u.get("status") in ("done", "partial") and u.get("pct", 0) > 0]
    print(("%.1f" % (sum(v) / len(v))) if v else "")

def cmd_table(path):
    print("| Unit | Status | Rounds | Codex tokens in/cached/out | Seconds | Mutants killed/survived/budgeted-out | Pack KB |")
    print("|---|---|---|---|---|---|---|")
    for k, v in sorted(state_load(path)["units"].items()):
        mut = "-" if "killed" not in v else "%d/%d/%d" % (v.get("killed", 0), v.get("survivors", 0), v.get("budgeted", 0))
        pk = "%.1f" % (v["pack_bytes"] / 1024) if v.get("pack_bytes") else "-"
        print("| %s | %s | %d | %d/%d/%d | %d | %s | %s |" % (k, v.get("status", ""), v.get("rounds", 0), v.get("tin", 0),
              v.get("tcached", 0), v.get("tout", 0), v.get("secs", 0), mut, pk))

# ------------------------------------------------------------------ tier / pack trimming
def cmd_tier(plan, uid):
    """skip (green or golden-covered), yellow or red - read from the plan row's Col and Test cells."""
    for line in read(plan).splitlines():
        if not re.match(r"^\|\s*" + re.escape(uid) + r"\s*\|", line):
            continue
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        col, test = c[3].lower(), c[5]
        if "\U0001F7E2" in col or "green" in col: return "skip"
        if re.match(r"^G\b", test): return "skip"
        if "\U0001F534" in col or "red" in col: return "red"
        return "yellow"
    return "yellow"

def trim_cs(text, cap):
    """Strip BOM, the leading license/header comment block, XML doc lines, trailing blanks and blank runs; cap the size."""
    lines = text.replace("﻿", "").replace("\r", "").split("\n")
    i = 0
    while i < len(lines):
        t = lines[i].strip()
        if t == "" or t.startswith("//"):
            i += 1
        elif t.startswith("/*"):
            while i < len(lines) and "*/" not in lines[i]: i += 1
            i += 1
        else:
            break
    out, blank = [], 0
    for l in lines[i:]:
        if l.lstrip().startswith("///"): continue
        l = l.rstrip()
        if l == "":
            blank += 1
            if blank > 1: continue
        else:
            blank = 0
        out.append(l)
    r = "\n".join(out).strip() + "\n"
    full = len(r.encode("utf-8"))
    if cap and full > cap:
        b = r.encode("utf-8")[:cap].decode("utf-8", "ignore")
        b = b[: b.rfind("\n") + 1] if "\n" in b else b
        r = b + "// ... [truncated by pack: file is %d bytes; read the rest with the git show command from the rules]\n" % full
    return r

def cmd_sigs(path, cap=200):
    seen, out = set(), []
    for l in read(path).splitlines():
        if re.match(r"^\s*(pub |impl\b|#\[derive)", l):
            k = l.strip()
            if k not in seen:
                seen.add(k); out.append(k)
    print("\n".join(out[:cap]))

# ------------------------------------------------------------------ codex log / quota
def cmd_codexparse(path, offset):
    """From the --json log (starting at byte offset): session id and token totals. Prints sid<TAB>in<TAB>cached<TAB>out."""
    sid, ti, tc, to = "", 0, 0, 0
    try:
        with open(path, "rb") as f:
            f.seek(int(offset)); data = f.read().decode("utf-8", "replace")
    except OSError:
        data = ""
    for line in data.splitlines():
        line = line.strip()
        if not line.startswith("{"): continue
        try: e = json.loads(line)
        except ValueError: continue
        if e.get("type") == "thread.started" and e.get("thread_id") and not sid: sid = e["thread_id"]
        if e.get("type") == "turn.completed":
            u = e.get("usage") or {}
            ti += int(u.get("input_tokens", 0)); tc += int(u.get("cached_input_tokens", 0)); to += int(u.get("output_tokens", 0))
    print("|".join([sid, str(ti), str(tc), str(to)]))

def cmd_used(text):
    try:
        j = json.loads(text.strip().splitlines()[0])
        print("%s|%s" % (j.get("five_hour_used", ""), j.get("five_hour_resets_at", "")))
    except Exception:
        print("|")

# ------------------------------------------------------------------ mutation helpers
def cmd_testargs(wt, contract):
    """cargo test selectors for the unit's own test binaries: --test <name> per owned tests/*.rs, --lib if a src file has #[cfg(test)]."""
    args, lib = [], False
    for f in owned(contract):
        p = os.path.join(wt, f)
        m = re.match(r"^crates/[^/]+/tests/([^/]+)\.rs$", f)
        if m and os.path.isfile(p): args += ["--test", m.group(1)]
        if re.match(r"^crates/[^/]+/src/.*\.rs$", f) and os.path.isfile(p) and "#[cfg(test)]" in read(p):
            lib = True
    if lib: args.append("--lib")
    print(" ".join(args))

def cmd_mutstat(d):
    """killed survived unviable timeouts budgeted-out (mutants listed but never tested, e.g. budget exhausted)."""
    def n(name):
        try: return len([l for l in read(os.path.join(d, "mutants.out", name)).splitlines() if l.strip()])
        except OSError: return 0
    try: total = len(json.loads(read(os.path.join(d, "mutants.out", "mutants.json"))))
    except Exception: total = 0
    k, sv, un, to = n("caught.txt"), n("missed.txt"), n("unviable.txt"), n("timeout.txt")
    print("%d %d %d %d %d" % (k, sv, un, to, max(0, total - k - sv - un - to)))

def cmd_mutregex(path):
    esc = lambda l: re.sub(r"([\\.+*?()|\[\]{}^$#&~-])", r"\\\1", l.strip())
    print("^(" + "|".join(esc(l) for l in read(path).splitlines()[:60] if l.strip()) + ")$")

# ------------------------------------------------------------------ deterministic review gates (no LLM)
def _git(wt, *a):
    r = subprocess.run(["git", "-C", wt] + list(a), capture_output=True, encoding="utf-8", errors="replace")
    return r.stdout if r.returncode == 0 else ""

def _crate(f):
    m = re.match(r"^crates/([^/]+)/", f); return m.group(1) if m else ""

def _code(l):
    return re.sub(r'"(?:[^"\\]|\\.)*"', '""', l).split("//")[0]

UNWRAP_CTX = re.compile(r"fs::|\bFile\b|\bPath(Buf)?\b|path|\bread|\bwrite|\bopen|\bdir|json|settings|journal|session|metadata|canonicalize|\.parent\(|\.file_name\(|\.to_str\(", re.I)

def cmd_lint(wt, ownedfile):
    own = [l.strip() for l in read(ownedfile).splitlines() if l.strip()]
    rs = [f for f in own if f.endswith(".rs") and os.path.isfile(os.path.join(wt, f))]
    bad = []
    diff = ""
    if rs:
        subprocess.run(["git", "-C", wt, "add", "-N", "--"] + rs, capture_output=True)
        diff = _git(wt, "diff", "HEAD", "-U0", "--no-color", "--", *rs)
    added, cur, ln = [], None, 0
    for l in diff.splitlines():
        if l.startswith("+++ "):
            cur = l[6:] if l.startswith("+++ b/") else None
        elif l.startswith("@@"):
            m = re.search(r"\+(\d+)", l); ln = int(m.group(1)) if m else 0
        elif l.startswith("+") and cur:
            added.append((cur, ln, l[1:])); ln += 1
    files = {f: read(os.path.join(wt, f)).replace("\r", "").split("\n") for f in rs}
    testfrom = {f: next((i + 1 for i, x in enumerate(v) if "#[cfg(test)]" in x), 10 ** 9) for f, v in files.items()}
    for f, n, t in added:
        code = _code(t); is_src = "/src/" in f; in_test = n >= testfrom.get(f, 10 ** 9)
        if is_src and not in_test:
            if re.search(r"\.unwrap\(\)|\.expect\(", code) and UNWRAP_CTX.search(code) and "allow-unwrap" not in t:
                bad.append("%s:%d unwrap()/expect( on a file-system/user-data path (return an error instead): %s" % (f, n, t.strip()[:100]))
            if re.search(r"\bunsafe\b", code) and _crate(f) != "pr-platform-win":
                bad.append("%s:%d `unsafe` outside pr-platform-win" % (f, n))
        if is_src and re.search(r"\bunsafe\s*\{|\bunsafe\s+impl\b", code):
            prev, i = [], n - 2
            while i >= 0 and (files[f][i].strip() == "" or files[f][i].lstrip().startswith(("//", "#["))):
                prev.append(files[f][i]); i -= 1
            if "SAFETY:" not in t and not any("SAFETY:" in x for x in prev):
                bad.append("%s:%d `unsafe` without a preceding `// SAFETY:` comment" % (f, n))
        if re.search(r"#!?\[allow\(", t):
            pv = files[f][n - 2] if 2 <= n <= len(files[f]) + 1 else ""
            if "reason" not in t and "//" not in t and not pv.lstrip().startswith("//"):
                bad.append("%s:%d `#[allow(...)]` without a reason comment: %s" % (f, n, t.strip()[:100]))
    for f in rs:
        if "/src/" not in f: continue
        for i, x in enumerate(files[f]):
            if i + 1 >= testfrom[f]: break
            if re.search(r"\b(todo|unimplemented)!\s*[\(\[{]", _code(x)):
                bad.append("%s:%d todo!()/unimplemented!() left in owned source" % (f, i + 1))
    for l in _git(wt, "status", "--porcelain", "-uall").splitlines():
        f = l[3:].split(" -> ")[-1].strip('"')
        if f.startswith(".claude/") or re.search(r"(^|/)target(-[^/]*)?/", f):
            bad.append("%s: `.claude`/target file in the change set (must never be staged)" % f)
    if bad:
        print("Static review gate violations (fix them in the owned files):")
        print("\n".join(bad[:40])); sys.exit(1)

FN_RE = re.compile(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?(?:async[ \t]+)?fn[ \t]+(\w+)")

def _fn_bodies(src):
    masked = re.sub(r'"(?:[^"\\\n]|\\.)*"|//[^\n]*', lambda m: " " * len(m.group(0)), src)
    res = {}
    for m in FN_RE.finditer(masked):
        i = masked.find("{", m.end())
        if i < 0: continue
        d, j = 0, i
        while j < len(masked):
            if masked[j] == "{": d += 1
            elif masked[j] == "}":
                d -= 1
                if d == 0: break
            j += 1
        body = [x.strip() for x in src[i:j + 1].split("\n") if x.strip() and not x.strip().startswith("//")]
        res.setdefault(m.group(1), body)
    return res

def cmd_testguard(wt, base, ownedfile):
    """Test files that existed at <base> may only grow: every old fn keeps all its old lines (in order); no ignore/should_panic added."""
    bad = []
    for f in [l.strip() for l in read(ownedfile).splitlines() if l.strip()]:
        if "/tests/" not in f or not f.endswith(".rs"): continue
        old = _git(wt, "show", "%s:%s" % (base, f)).replace("\r", "")
        if not old: continue
        pth = os.path.join(wt, f)
        if not os.path.isfile(pth):
            bad.append("%s: red/test file deleted" % f); continue
        new = read(pth).replace("\r", "")
        ob, nb = _fn_bodies(old), _fn_bodies(new)
        for name, lines in ob.items():
            if name not in nb:
                bad.append("%s: test fn `%s` removed or renamed" % (f, name)); continue
            it = iter(nb[name])
            if not all(any(x == y for y in it) for x in lines):
                bad.append("%s: test fn `%s` was modified/weakened (only additions are allowed)" % (f, name))
        for pat in (r"#\[ignore", r"#\[should_panic"):
            if len(re.findall(pat, new)) > len(re.findall(pat, old)): bad.append("%s: new `%s` attribute added" % (f, pat[2:]))
        if len(re.findall(r"#\[test\]", new)) < len(re.findall(r"#\[test\]", old)): bad.append("%s: number of #[test] decreased" % f)
    if bad:
        print("Red/test files of the red-tests commit must not be modified or weakened:")
        print("\n".join(bad[:30])); sys.exit(1)

# ------------------------------------------------------------------ failure triage (<= 3 KB)
def cmd_triage(out, uid, reason, errfile, wt, lastmsg):
    def tailb(t, n):
        b = t.encode("utf-8")
        return b[-n:].decode("utf-8", "ignore") if len(b) > n else t
    err = read(errfile).replace("\r", "") if errfile and os.path.isfile(errfile) else ""
    tests = sorted(set(re.findall(r"^test (\S+) \.\.\. FAILED", err, re.M) + re.findall(r"^---- (\S+) stdout ----", err, re.M)))
    stat = _git(wt, "diff", "--stat", "HEAD") if wt and os.path.isdir(wt) else ""
    last = read(lastmsg) if lastmsg and os.path.isfile(lastmsg) else ""
    parts = ["# %s needs Claude\n\n" % uid, "Reason: %s\n\n" % reason[:300],
             "## Error tail\n```\n%s\n```\n" % tailb("\n".join(err.splitlines()[-40:]), 1100),
             "## git diff --stat\n```\n%s\n```\n" % stat[:450],
             "## Failing tests\n%s\n\n" % (", ".join(tests)[:300] or "(none parsed)"),
             "## Last Codex message\n```\n%s\n```\n" % last[:450]]
    txt = "".join(parts).encode("utf-8")[:3072].decode("utf-8", "ignore")
    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as f: f.write(txt)

def cmd_members():
    """Workspace members of the cwd: name<TAB>edition<TAB>manifest dir (from `cargo metadata --no-deps`)."""
    out = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"], stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, check=True).stdout.decode("utf-8", "replace")
    for pk in json.loads(out)["packages"]:
        print("	".join([pk["name"], pk.get("edition", "2021"), os.path.dirname(pk["manifest_path"])]))

def cmd_sreset(path, uid):
    """Forget a unit's state and telemetry (rerun of a failed unit starts clean)."""
    with state_lock(path):
        s = state_load(path); s["units"].pop(uid, None); state_save(path, s)

def cmd_redcheck(path):
    """Red-test run check: every failing test must fail ONLY through todo!()/unimplemented!() of the stubs.
    Exit 0 = ok; 1 = prints the offending tests (compile errors, assertion failures, other panics)."""
    t = read(path)
    bad = []
    if re.search(r"^error\[E\d+\]|error: could not compile", t, re.M):
        bad.append("compile error in the test output")
    blocks = re.split(r"^---- (.+?) stdout ----$", t, flags=re.M)
    seen = 0
    for i in range(1, len(blocks) - 1, 2):
        seen += 1
        name, body = blocks[i], blocks[i + 1].split(chr(10) + "failures:")[0]
        if not re.search(r"not yet implemented|not implemented", body):
            first = next((l for l in body.splitlines() if l.strip() and "panicked at" not in l and "note:" not in l), "")
            bad.append("%s fails but not through todo!(): %s" % (name, first.strip()[:160]))
    nfail = len(re.findall(r"^test .* \.\.\. FAILED$", t, re.M))
    if nfail > seen:
        bad.append("%d failing test(s) without panic output (crash or timeout)" % (nfail - seen))
    if bad:
        print("RED TESTS ARE NOT VALID: they must compile and fail only because the stubs call todo!(). Problems:")
        print(chr(10).join("- " + b for b in bad[:30])); sys.exit(1)

def main(a):
    c = a[0]
    if c == "order": cmd_order(a[1], a[2], a[3] if len(a) > 3 else None)
    elif c == "row": cmd_row(a[1], a[2])
    elif c == "deps": cmd_deps(a[1], a[2])
    elif c == "owned": print("\n".join(owned(a[1])))
    elif c == "cs": cmd_cs(a[1])
    elif c == "baseline": cmd_baseline(a[1])
    elif c == "sget":      # sget file id -> status (or empty)
        print(state_load(a[1])["units"].get(a[2], {}).get("status", ""))
    elif c == "sfield":    # sfield file id key -> value (or empty)
        print(state_load(a[1])["units"].get(a[2], {}).get(a[3], ""))
    elif c == "sset": cmd_sset(*a[1:6])   # sset file id status [note] [survivors]
    elif c == "supd": cmd_supd(a[1], a[2], a[3:])
    elif c == "slist":     # slist file -> id<TAB>status<TAB>survivors<TAB>note
        for k, v in sorted(state_load(a[1])["units"].items()):
            print("\t".join([k, v.get("status", ""), str(v.get("survivors", 0)), v.get("note", "")]))
    elif c == "table": cmd_table(a[1])
    elif c == "avgpct": cmd_avgpct(a[1])
    elif c == "tier": print(cmd_tier(a[1], a[2]))
    elif c == "trimcs": sys.stdout.write(trim_cs(sys.stdin.buffer.read().decode("utf-8", "replace"), int(a[1]) if len(a) > 1 else 0))
    elif c == "sigs": cmd_sigs(a[1])
    elif c == "codexparse": cmd_codexparse(a[1], a[2])
    elif c == "used": cmd_used(sys.stdin.read())
    elif c == "testargs": cmd_testargs(a[1], a[2])
    elif c == "mutstat": cmd_mutstat(a[1])
    elif c == "mutregex": cmd_mutregex(a[1])
    elif c == "lint": cmd_lint(a[1], a[2])
    elif c == "testguard": cmd_testguard(a[1], a[2], a[3])
    elif c == "members": cmd_members()
    elif c == "sreset": cmd_sreset(a[1], a[2])
    elif c == "redcheck": cmd_redcheck(a[1])
    elif c == "triage": cmd_triage(a[1], a[2], a[3], a[4], a[5], a[6] if len(a) > 6 else "")
    else:
        sys.exit("unknown command " + c)

if __name__ == "__main__":
    main(sys.argv[1:])
