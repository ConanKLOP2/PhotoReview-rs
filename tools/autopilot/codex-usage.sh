#!/usr/bin/env bash
# Prints Codex 5h/weekly usage as one JSON line, and exits 0 when Codex may be used (5h window < THRESHOLD %, no limit reached), 1 otherwise.
# Usage: tools/autopilot/codex-usage.sh [threshold_percent=95]
C="${CODEX_BIN:-$HOME/.codex/plugins/.plugin-appserver/codex.exe}"
TH="${1:-95}"
OUT=$( (printf '%s\n' '{"id":1,"method":"initialize","params":{"clientInfo":{"name":"autopilot-usage","title":null,"version":"1"}}}' '{"method":"initialized"}'; sleep 2; printf '%s\n' '{"id":2,"method":"account/rateLimits/read"}'; sleep 10) | timeout 25 "$C" app-server 2>/dev/null | grep '"id":2')
[ -z "$OUT" ] && { echo '{"ok":false,"reason":"no response"}'; exit 1; }
printf '%s' "$OUT" | python -c "
import sys,json,time
r=json.loads(sys.stdin.read())['result']; p=r['rateLimits']['primary']; s=r['rateLimits']['secondary']
ok=r.get('ordinaryUsageAllowed',False) and not r['rateLimits'].get('rateLimitReachedType') and p['usedPercent']<$TH
print(json.dumps({'ok':ok,'five_hour_used':p['usedPercent'],'five_hour_resets_at':time.strftime('%Y-%m-%d %H:%M',time.localtime(p['resetsAt'])),'weekly_used':s['usedPercent'],'weekly_resets_at':time.strftime('%Y-%m-%d %H:%M',time.localtime(s['resetsAt']))}))
sys.exit(0 if ok else 1)"
