# Claude Code hooks

`post-edit-rustfmt.sh` formats in-repository Rust files after Edit/Write. Formatting failures are swallowed so incomplete edits remain usable.
`pre-bash-guard.sh` blocks broad formatting, cross-lane process kills, protected force pushes, dangerous recursive removals, and unscoped `git clean`.

Status (checked 2026-10-26): NOT registered; the hooks are only documented here. The owner registers them after review; no agent or PR applies settings. Add the following to `.claude/settings.local.json`:

```json
{
  "hooks": {
    "PostToolUse": [{"matcher":"Edit|Write","hooks":[{"type":"command","command":"bash tools/dev/hooks/post-edit-rustfmt.sh"}] }],
    "PreToolUse": [{"matcher":"Bash","hooks":[{"type":"command","command":"bash tools/dev/hooks/pre-bash-guard.sh"}] }]
  }
}
```

To remove them, delete those two entries from `hooks` in `.claude/settings.local.json`.
