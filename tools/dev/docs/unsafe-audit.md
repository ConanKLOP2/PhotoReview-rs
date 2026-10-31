# unsafe-audit.sh

Purpose: read-only hygiene scan of `crates/**/src` and `crates/**/tests` (target excluded) for `unsafe`.

Usage: `bash tools/dev/unsafe-audit.sh` ; `bash tools/dev/unsafe-audit.sh --update-baseline` (rewrites `tools/dev/unsafe-audit.baseline`).

Rules: (a) every unsafe block/fn/impl/trait/extern needs `SAFETY:` in the 3 lines above, on the same line or in the first 2 lines inside (unsafe fn/extern also accept a `# Safety` doc section); (b) unsafe only in the allowed crates (AGENTS.md FFI list: pr-platform-win, pr-turbojpeg, pr-libraw; the codex gate is stricter for new lines: only pr-platform-win); (c) report-only unwrap()/expect(/panic! counts of pr-platform-win non-test code. Exit 1 on (a)/(b) findings not in the baseline. Policy note (P-64, `docs/adr/ADR-D-2026-10-31-unsafe-policy.md`): the 9 production lines of `pr-core` and 6 of `pr-imaging` in the baseline (rule B) are grandfathered and may only shrink; new `unsafe` there needs an ADR. The script and baseline are unchanged by that decision. Baseline keys are rule + path + normalized unsafe line, so they survive line shifts; duplicates count one per entry.

Env overrides: `PY_BIN`, `UNSAFE_AUDIT_ROOT`, `UNSAFE_AUDIT_BASELINE`, `UNSAFE_ALLOWED_CRATES`.

Safety: reads files only (writes the baseline file only with `--update-baseline`). The baseline should only shrink.

CI: the `unsafe-audit` job in `.github/workflows/ci.yml` (ubuntu, no toolchain) runs this script on every pull request and `build-test` requires it.
