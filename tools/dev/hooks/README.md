# Claude Code hooks

`post-edit-rustfmt.sh` formats in-repository Rust files after Edit/Write. Formatting failures are swallowed so incomplete edits remain usable.
`pre-bash-guard.sh` blocks broad formatting, cross-lane process kills, protected force pushes, dangerous recursive removals, and unscoped `git clean`.

The lead registers these after review; this change does **not** apply settings. Add the following to `.claude/settings.local.json`:

```json
{
  "hooks": {
    "PostToolUse": [{"matcher":"Edit|Write","hooks":[{"type":"command","command":"bash tools/dev/hooks/post-edit-rustfmt.sh"}] }],
    "PreToolUse": [{"matcher":"Bash","hooks":[{"type":"command","command":"bash tools/dev/hooks/pre-bash-guard.sh"}] }]
  }
}
```

To remove them, delete those two entries from `hooks` in `.claude/settings.local.json`.
