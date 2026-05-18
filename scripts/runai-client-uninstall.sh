#!/usr/bin/env bash
# runai client uninstall — reverses everything `runai-client-install.sh`
# did: removes the UserPromptSubmit hook entry pointing at ~/.runai-hook.sh
# from ~/.claude/settings.json, then deletes the hook script itself.
#
# Usage:
#   curl -fsSL http://<SERVER>:<PORT>/uninstall | bash
#
# Safe to run if you never installed — both steps are no-ops in that case.

set -euo pipefail

HOOK_PATH="$HOME/.runai-hook.sh"
SETTINGS_PATH="$HOME/.claude/settings.json"

echo "runai client uninstall"
echo

# 1) Remove the hook entry from settings.json (if present). Idempotent.
if [[ -f "$SETTINGS_PATH" ]]; then
  cp "$SETTINGS_PATH" "${SETTINGS_PATH}.runai-uninstall-bak"
  python3 - "$SETTINGS_PATH" "$HOOK_PATH" <<'PY'
import json
import sys

settings_path = sys.argv[1]
hook_path = sys.argv[2]

with open(settings_path) as f:
    try:
        data = json.load(f)
    except json.JSONDecodeError:
        print('settings.json was not valid JSON — leaving untouched')
        sys.exit(0)

hooks = data.get('hooks', {})
ups = hooks.get('UserPromptSubmit', [])

removed = 0
new_ups = []
for group in ups:
    inner = group.get('hooks', [])
    kept = [h for h in inner if h.get('command') != hook_path]
    if not kept:
        # whole group was ours — drop the wrapper too
        removed += len(inner)
        continue
    if len(kept) != len(inner):
        removed += len(inner) - len(kept)
    new_ups.append({**group, 'hooks': kept})

if new_ups:
    hooks['UserPromptSubmit'] = new_ups
elif 'UserPromptSubmit' in hooks:
    del hooks['UserPromptSubmit']

with open(settings_path, 'w') as f:
    json.dump(data, f, indent=2, ensure_ascii=False)
    f.write('\n')

print(f'removed {removed} runai hook entr{"y" if removed == 1 else "ies"} from settings.json')
PY
else
  echo "no settings.json — nothing to clean"
fi

# 2) Delete the hook wrapper script itself.
if [[ -f "$HOOK_PATH" ]]; then
  rm -f "$HOOK_PATH"
  echo "removed $HOOK_PATH"
else
  echo "no $HOOK_PATH — already clean"
fi

echo
echo "done. Claude Code will no longer call runai on UserPromptSubmit."
echo "your original settings.json was backed up to ${SETTINGS_PATH}.runai-uninstall-bak"
