"""unsafe-audit helper. usage: unsafe-audit.py ROOT BASELINE [--update-baseline]

Rules: (a) unsafe block/fn/impl/trait/extern without a SAFETY comment, (b) unsafe outside the crates that may hold it,
(c) report-only unwrap/expect/panic counts in non-test code of pr-platform-win.
Exit 1 on (a)/(b) findings that are not in the baseline. Read-only except --update-baseline."""
import os
import re
import sys
from collections import Counter

# AGENTS.md: unsafe is FFI only (WIC/COM, Shell in pr-platform-win; TurboJPEG, LibRaw in their crates).
# tools/autopilot/lib/codex-lib.py (cmd_lint) additionally rejects unsafe outside pr-platform-win in NEW lines;
# the existing, baselined code in other crates is listed in the baseline file.
ALLOWED = set(os.environ.get("UNSAFE_ALLOWED_CRATES", "pr-platform-win,pr-turbojpeg,pr-libraw").split(","))
COUNT_CRATE = os.environ.get("UNSAFE_COUNT_CRATE", "pr-platform-win")
RAW_START = re.compile(r'(?:br|rb|r)(#*)"')


def blank(text):
    return "".join(ch if ch == "\n" else " " for ch in text)


def mask(src):
    """src with comments and string/char literal contents replaced by spaces (newlines and length kept)."""
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        two = src[i:i + 2]
        prev_word = i > 0 and (src[i - 1].isalnum() or src[i - 1] == "_")
        m = None if prev_word or c not in "rb" else RAW_START.match(src, i)
        if two == "//":
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif two == "/*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src[j:j + 2] == "/*":
                    depth += 1
                    j += 2
                elif src[j:j + 2] == "*/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            out.append(blank(src[i:j]))
            i = j
        elif m:
            close = '"' + m.group(1)
            end = src.find(close, m.end())
            end = n if end < 0 else end + len(close)
            out.append(blank(src[i:end]))
            i = end
        elif c == '"' or (c == "b" and two == 'b"' and not prev_word):
            j = i + (2 if c == "b" else 1)
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            j = min(j + 1, n)
            out.append('"' + blank(src[i + 1:j - 1]) + '"' if c == '"' else ' "' + blank(src[i + 2:j - 1]) + '"')
            i = j
        elif c == "'":
            lit = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|.)|[^\\'\n])'").match(src, i)
            if lit:
                out.append(blank(src[i:lit.end()]))
                i = lit.end()
            else:
                out.append(c)
                i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def norm(s):
    return re.sub(r"\s+", " ", s.strip())


def crate_of(rel):
    m = re.match(r"crates/([^/]+)/", rel)
    return m.group(1) if m else ""


def kinds_on_line(code, i):
    """Kinds of unsafe constructs starting on masked line i. A kind is block, impl, trait, fn, extern or other."""
    line = code[i]
    kinds = []
    for m in re.finditer(r"\bunsafe\b", line):
        rest, j = line[m.end():], i
        while not rest.strip() and j + 1 < len(code) and j - i < 3:
            j += 1
            rest = code[j]
        r = rest.strip()
        if r.startswith("{"):
            kinds.append("block")
        elif re.match(r"impl\b", r):
            kinds.append("impl")
        elif re.match(r"trait\b", r):
            kinds.append("trait")
        elif re.match(r'extern\s*(?:"[^"]*"\s*)?\{', r):
            kinds.append("extern")
        elif re.match(r'(?:extern\s*(?:"[^"]*"\s*)?)?fn\s*\(', r):
            continue  # fn pointer type, not a declaration
        elif re.match(r'(?:extern\s*(?:"[^"]*"\s*)?)?fn\b', r):
            kinds.append("fn")
        else:
            kinds.append("other")
    if not kinds and re.match(r'\s*(?:pub(?:\([^)]*\))?\s+)?extern\s*(?:"[^"]*"\s*)?\{', line):
        kinds.append("extern")  # old-style foreign block (not unsafe extern)
    return kinds


def has_safety(raw, i, kind):
    """SAFETY: within the 3 lines above, on the same line or in the first 2 lines inside the block/body (the repo's
    `unsafe {` + `// SAFETY:` style); fn/extern/trait/impl also accept a '# Safety' doc section."""
    if any("SAFETY:" in w for w in raw[max(0, i - 3):i + 3]):
        return True
    if kind in ("fn", "extern", "trait", "impl"):
        k = i - 1
        while k >= 0 and raw[k].lstrip().startswith(("//", "#[", "#![")):
            if "# Safety" in raw[k] or "SAFETY:" in raw[k]:
                return True
            k -= 1
    return False


def scan_files(root):
    found = []
    for dirpath, dirnames, filenames in os.walk(os.path.join(root, "crates")):
        dirnames[:] = [d for d in dirnames if d != "target"]
        rel_dir = os.path.relpath(dirpath, root).replace("\\", "/")
        for fn in filenames:
            parts = (rel_dir + "/" + fn).split("/")
            if fn.endswith(".rs") and len(parts) >= 4 and parts[2] in ("src", "tests"):
                found.append("/".join(parts))
    return sorted(found)


def read_text(path):
    with open(path, "rb") as f:
        return f.read().decode("utf-8", "replace").replace("\r\n", "\n")


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    update = "--update-baseline" in sys.argv
    root, baseline_path = args[0], args[1]
    files = scan_files(root)
    findings, counts = [], {}
    for rel in files:
        text = read_text(os.path.join(root, rel))
        raw = text.split("\n")
        code = mask(text).split("\n")
        crate = crate_of(rel)
        for i, line in enumerate(code):
            if "unsafe" not in line and "extern" not in line:
                continue
            for kind in kinds_on_line(code, i):
                key_text = norm(raw[i])
                if not has_safety(raw, i, kind):
                    findings.append(("A", rel, key_text, i + 1, kind))
                if crate not in ALLOWED and re.search(r"\bunsafe\b", line):
                    findings.append(("B", rel, key_text, i + 1, kind))
        if crate == COUNT_CRATE and "/src/" in rel and not re.search(r"(?:^|/)tests?\.rs$|_tests?\.rs$", rel):
            cut = next((k for k, l in enumerate(code) if "#[cfg(test)]" in l), len(code))
            body = "\n".join(code[:cut])
            counts[rel] = [len(re.findall(r"\.unwrap\(\)", body)), len(re.findall(r"\.expect\(", body)), len(re.findall(r"\bpanic!", body))]
    entries = Counter("%s\t%s\t%s" % (r, p, t) for r, p, t, _, _ in findings)
    if update:
        with open(baseline_path, "w", encoding="utf-8", newline="\n") as f:
            f.write("# unsafe-audit baseline. Format: RULE<TAB>path<TAB>normalized unsafe line (no line numbers). A = no SAFETY comment, B = unsafe outside allowed crates.\n")
            f.write("# Regenerate with: bash tools/dev/unsafe-audit.sh --update-baseline. It should only shrink.\n")
            for k in sorted(entries):
                for _ in range(entries[k]):
                    f.write(k + "\n")
        print("baseline written: %d entries (%s)" % (sum(entries.values()), baseline_path))
        return 0
    base = Counter()
    if os.path.isfile(baseline_path):
        for line in read_text(baseline_path).split("\n"):
            if line.strip() and not line.startswith("#"):
                base[line] += 1
    remaining, new, baselined = Counter(base), [], 0
    for r, p, t, ln, kind in findings:
        k = "%s\t%s\t%s" % (r, p, t)
        if remaining[k] > 0:
            remaining[k] -= 1
            baselined += 1
        else:
            new.append((r, p, ln, kind, t))
    na = sum(1 for f in findings if f[0] == "A")
    nb = sum(1 for f in findings if f[0] == "B")
    print("unsafe-audit: %d files scanned; allowed crates: %s" % (len(files), ",".join(sorted(ALLOWED))))
    print("(a) unsafe without SAFETY / '# Safety': %d found" % na)
    print("(b) unsafe outside allowed crates: %d found" % nb)
    print("baselined: %d finding(s) covered by %d baseline entries in %s (unused baseline entries: %d)"
          % (baselined, sum(base.values()), os.path.basename(baseline_path), sum(remaining.values())))
    tu, te, tp = (sum(c[x] for c in counts.values()) for x in range(3))
    print("(c) %s non-test code, report only: unwrap()=%d expect(=%d panic!=%d (%d files)" % (COUNT_CRATE, tu, te, tp, len(counts)))
    for rel, c in sorted(counts.items(), key=lambda kv: -sum(kv[1]))[:5]:
        if sum(c):
            print("    %-66s unwrap=%d expect=%d panic=%d" % (rel, c[0], c[1], c[2]))
    if new:
        print("NEW VIOLATIONS (%d):" % len(new))
        for r, p, ln, kind, t in new:
            msg = "no SAFETY comment in the 3 lines above" if r == "A" else "unsafe outside the allowed crates"
            print("  [%s] %s:%d (%s) %s: %s" % (r, p, ln, kind, msg, t[:100]))
        return 1
    print("unsafe-audit OK: no new violations")
    return 0


sys.exit(main())
