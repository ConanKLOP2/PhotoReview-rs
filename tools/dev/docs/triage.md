# triage.sh

Purpose: classify a failed autopilot unit and suggest the next action. Read-only; it never starts an agent.

```bash
bash tools/dev/triage.sh W8-18b            # class, evidence lines, next action
bash tools/dev/triage.sh W8-18b --prompt p.md   # also write a ready prompt for an AI coding agent
bash tools/dev/triage.sh --all             # one line per needs-claude note
```

Reads `tools/autopilot/state/needs-claude/<unit>.md`, `logs/<unit>.gate-error.txt`, `logs/<unit>*.log`
and `contracts/<unit>.md` (fenced "Owned files" list, used only for --prompt).
Classes: environment, compile-error, red-test-guard, stubs-left, test-failure, rustfmt-only, mutants-budget, unknown.
Missing files are reported as "nothing to triage" (a clean checkout has no state).

Env overrides: `STATE_DIR`, `CONTRACTS_DIR`, `PY_BIN`.
Safety: only reads state; writes only the --prompt file you name. The prompt carries the last 60 error lines and the
rules: never weaken a test, cite the C# source when correcting one, UTF-8 LF no BOM.
Selftest: `bash tools/dev/test/triage-selftest.sh`.
