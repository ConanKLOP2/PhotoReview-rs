---
name: unsafe-audit
description: Use when the user asks to audit unsafe code, SAFETY comments, or unwrap/panic counts, or before a PR that touches unsafe.
---

Run `bash tools/dev/unsafe-audit.sh` (read-only, about 20 seconds).
Report in Vietnamese in at most 10 lines: the found and baselined counts for (a) missing SAFETY comments and (b) unsafe outside allowed crates,
any NEW VIOLATIONS with file:line and what to fix, and the unwrap/expect/panic counts of pr-platform-win non-test code.
Fix new violations by adding a `// SAFETY:` comment; never add entries to `tools/dev/unsafe-audit.baseline` by hand.
Run `--update-baseline` only if the user explicitly asks, and say how many entries it changed.

Run every command from the repository root.
