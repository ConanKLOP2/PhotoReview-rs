# oracle.sh

Reproducibility check around `tools/oracle.ps1` (regenerates `tests/golden/<cmd>.json` from the C# source).

```bash
bash tools/dev/oracle.sh natural-cmp dotnet-path --runs 2 --commit <sha> --keep
```

Per command it runs `powershell -NoProfile -ExecutionPolicy Bypass -File tools/oracle.ps1 [-Commit <sha>] -Command <cmd>` N times
(default 2), copies the golden after each run, compares sha256 of the copies (identical / DIFFERENT) and compares with
`git show HEAD:tests/golden/<cmd>.json` (says when only `csharp_commit` differs). Warns when turbojpeg.dll / libraw.dll are missing.
Exit 0 only when everything is identical and no run failed; 2 on bad usage.

Env: `PS_BIN`, `GIT_BIN`, `ORACLE_CSHARP_REPO` (native/x64 lookup), `ORACLE_TEMP` (pr-oracle cache lookup).

Safety: restores the committed goldens afterwards (a golden that did not exist is removed) unless `--keep`; refuses to start when
a golden has local changes. Command names are restricted to `[A-Za-z0-9._-]`. This runs the real C# oracle (slow, builds C#): the
selftest `bash tools/dev/test/oracle-selftest.sh` uses fakes only, in a temporary repo root.
