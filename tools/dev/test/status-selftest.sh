#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"; tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
mkdir -p "$tmp/bin"; cat > "$tmp/bin/git.sh" <<'EOF'
#!/bin/sh
case "$*" in *for-each-ref*) echo 'u1|machine-a';; *'worktree list'*) echo "worktree /fake/main";; *status*) :;; *fetch*) exit 0;; esac
case "$*" in *--version*) echo 'git version fake';; esac
EOF
cat > "$tmp/bin/gh.sh" <<'EOF'
#!/bin/sh
echo '[]'
EOF
cat > "$tmp/bin/ps.sh" <<'EOF'
#!/bin/sh
case "$*" in *Get-CimInstance*) echo '[{"Name":"cargo-mutants.exe","CommandLine":"cargo-mutants"},{"Name":"cargo.exe","CommandLine":"cargo"},{"Name":"bash.exe","CommandLine":"bash tools/autopilot/codex-pipeline.sh"}]';; *) echo 10;; esac
EOF
cat > "$tmp/bin/usage.sh" <<'EOF'
#!/bin/sh
echo '{"ok":false,"five_hour_used":96,"five_hour_resets_at":"soon","weekly_used":95,"weekly_resets_at":"later"}'
EOF
chmod +x "$tmp/bin"/*
export GIT_BIN="$tmp/bin/git.sh" GH_BIN="$tmp/bin/gh.sh" PS_BIN="$tmp/bin/ps.sh" USAGE_BIN="$tmp/bin/usage.sh" PR_MACHINE=machine-a
text=$(bash "$ROOT/tools/dev/status.sh" --json 2>/dev/null)
if printf '%s' "$text" | python -c 'import json,sys; d=json.load(sys.stdin); assert all(k in d for k in ("quota","disk_free_gb","processes","pipelines","claims","pull_requests","worktrees")); assert d["quota"]["status"]=="STOP" and d["quota"]["ok"] is False; assert d["processes"]["cargo"]==1 and d["processes"]["cargo-mutants"]==1'; then ok 'JSON shape, quota threshold, and exact process categories'; else bad 'JSON shape, quota threshold, and exact process categories'; fi
plain=$(bash "$ROOT/tools/dev/status.sh" 2>/dev/null)
for s in QUOTA DISK PROCESSES CLAIMS 'PULL REQUESTS' WORKTREES; do case "$plain" in *"$s"*) :;; *) bad "section $s";; esac; done
case "$plain" in *'STOP'*'LOW DISK'*) ok 'quota STOP and low disk warning';; *) bad 'quota STOP and low disk warning';; esac
if [[ "$plain" == *'cargo-mutants=1'* && "$plain" == *'cargo=1'* && "$plain" == *'codex-pipeline.sh=1'* ]]; then ok 'process categories do not overlap'; else bad 'process categories do not overlap'; fi
case "$plain" in *'machine-a=1'*'CLAIMS HELD BY machine-a: u1'*) ok 'claim counts and owned units';; *) bad 'claim counts and owned units';; esac
case "$plain" in *"{'"* ) bad 'human output is not Python repr';; *) ok 'human readable text';; esac
if GIT_BIN="$tmp/no-git" GH_BIN="$tmp/no-gh" PS_BIN="$tmp/no-ps" bash "$ROOT/tools/dev/status.sh" --json 2>/dev/null | python -c 'import json,sys; d=json.load(sys.stdin); assert d["claims"]=="n/a" and d["pull_requests"]=="n/a" and d["disk_free_gb"] is None'; then ok 'n/a fallbacks'; else bad 'n/a fallbacks'; fi
rm -rf "$tmp"; if [ "$fails" -eq 0 ]; then echo 'selftest PASSED'; else echo "selftest $fails FAILED"; fi; exit "$fails"
