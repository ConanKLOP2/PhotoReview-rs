#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
PY="${PY_BIN:-python}"
fails=0
check() { if [ "$1" = "$2" ]; then echo "PASS: $3"; else echo "FAIL: $3 expected=$2 actual=$1"; fails=$((fails+1)); fi; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/crates"
# Use an existing in-repository directory so the hook's containment check runs.
fake="$tmp/rustfmt"
cat > "$fake" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$FMT_LOG"
if [ "${FMT_RC:-0}" -ne 0 ]; then echo 'error: expected item, found incomplete input' >&2; exit "$FMT_RC"; fi
EOF
chmod +x "$fake"
export FMT_LOG="$tmp/log" RUSTFMT_BIN="$fake"
payload() { "$PY" -c 'import json,sys; print(json.dumps({"tool_input":{sys.argv[1]:sys.argv[2]}}))' "$1" "$2"; }
msg=$(payload file_path "$ROOT/crates/bad.rs" | bash "$ROOT/tools/dev/hooks/post-edit-rustfmt.sh" 2>&1); check "$?" 0 'rustfmt hook'
grep -q -- '--edition 2024' "$FMT_LOG"; check "$?" 0 'rustfmt invoked with edition'
before=$(wc -l < "$FMT_LOG")
payload file_path "$ROOT/README.md" | bash "$ROOT/tools/dev/hooks/post-edit-rustfmt.sh"; check "$?" 0 'non-rust'
payload file_path "$tmp/outside.rs" | bash "$ROOT/tools/dev/hooks/post-edit-rustfmt.sh"; check "$?" 0 'outside'
check "$(wc -l < "$FMT_LOG")" "$before" 'non-Rust and outside files never invoke formatter'
export FMT_RC=1
msg=$(payload file_path "$ROOT/crates/bad.rs" | bash "$ROOT/tools/dev/hooks/post-edit-rustfmt.sh" 2>&1); check "$?" 0 'parse error exits zero'
check "$msg" '' 'parse error is silent'
check "$(wc -l < "$FMT_LOG")" "$((before+1))" 'parse-error formatter actually invoked'
guard="$ROOT/tools/dev/hooks/pre-bash-guard.sh"
guard_case() {
  local expected="$1" text="$2" msg rc
  msg=$(payload command "$text" | bash "$guard" 2>&1); rc=$?
  check "$rc" "$expected" "$text"
  if [ "$expected" = 2 ]; then [ -n "$msg" ]; check "$?" 0 'blocked command explains remedy'
  else check "$msg" '' 'allowed command silent'; fi
}
for text in  'cargo fmt --all' 'cargo fmt && cargo test -p x' "bash -c 'cargo fmt --all'"  "sh -c 'cargo fmt --all'" 'cmd /c "cargo fmt --all"'  'VAR=x command env X=y sudo time nohup cargo fmt --all' 'command -p time -p cargo fmt --all' 'sudo -u root cargo fmt --all'  'cargo fmt -- --package pr-core' 'echo ok; cargo fmt'  'taskkill //F //IM rustc.exe' 'taskkill.exe /im CARGO.EXE /f'  'git push --force origin master' 'git push origin +HEAD:refs/heads/main'  'git push --force-with-lease origin HEAD:integration/x'  'rm -rf .' 'rm -rf ./crates/..' 'rm -fr C:/MyProjects' 'rm -r -f /c/MyProjects/' 'rm -Rf .git/'  'rm --recursive --force .claude' 'rm -rf "$HOME"' 'rm -rf ~/'  'Remove-Item -Recurse .claude/worktrees' 'ri -Recurse "C:/MyProjects/"'  'git clean -fdx' 'git clean -f -d' 'git clean -fd --'
do guard_case 2 "$text"; done
for text in  'git commit -m "never run cargo fmt --all"' 'echo "cargo fmt"'  'gh pr create --body "... taskkill /IM rustc.exe ..."'  'git commit -m "cargo fmt --all"' 'cargo fmt -p pr-core -- --check'  'git clean -fd -- target' 'git clean -fd target'  'rm -rf .claude/worktrees/x/target' 'git push origin HEAD:sync/x'  'taskkill /F /PID 123' 'echo ";" "cargo" "fmt"'  'echo "a && cargo fmt"' 'cargo fmt -p pr-core; echo cargo fmt'  'git push --force origin feature' 'cargo fmt "unterminated'
do guard_case 0 "$text"; done
guard_case 0 'echo "cargo fmt ; taskkill /IM rustc.exe"'
guard_case 0 '# ; cargo fmt --all'
guard_case 2 $'echo ok # comment\ncargo fmt --all'
guard_case 0 $'cat <<EOF\ncargo fmt --all\nEOF\n'
guard_case 0 $'cat <<\'EOF\'\ntaskkill /IM rustc.exe\nEOF\n'
guard_case 0 $'cat <<-EOF\n\tcargo fmt --all\n\tEOF\n'
guard_case 2 $'cat <<EOF\ncargo fmt -p x\nEOF\ncargo fmt --all'
guard_case 2 $'echo "cargo fmt"\ncargo fmt'
printf 'not json' | bash "$guard"; check "$?" 0 'invalid payload fails open'
if [ "$fails" -eq 0 ]; then echo 'selftest PASSED'; else echo "selftest $fails FAILED"; fi
exit "$fails"
