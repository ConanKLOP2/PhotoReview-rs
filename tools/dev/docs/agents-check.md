# agents-check

Purpose: validate the project's Claude Code agent definitions in `.claude/agents/*.md` (contract-writer,
contract-reviewer, win32-reviewer, invariant-auditor, csharp-spec).

```bash
bash tools/dev/agents-check.sh                 # checks .claude/agents
bash tools/dev/agents-check.sh <agents-dir>    # path relative to the repo root
```

Checks per file: UTF-8 without BOM, LF; frontmatter is valid YAML (PyYAML if installed, else a strict
mini parser that rejects unquoted `: `); `name` equals the file name; `description` non-empty; `tools` present
and from the known tool set; `model` set; the prompt is not empty. Every backticked path whose first part is a
repo root (`contracts`, `docs`, `crates`, `tools`, `tests`, `assets`, `.claude`, `.github`) or that is a known root
file must exist (a `:line` suffix is ignored; placeholders with `<>` or `*` and `::` Rust paths are skipped).

Env: `PY_BIN` (python; defaults to `python`, then `python3`).
Output: `ok <agent> ...` / `FAIL <agent>: ...`, then `agents-check PASSED` or `agents-check N FAILED` (exit 1).
Safety: read-only, writes nothing, never touches the C# repo; python runs with `-I`.
