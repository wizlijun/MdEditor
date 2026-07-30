#!/bin/sh
# Is this run worth starting? Answering a note with nothing open costs tokens
# and tells you nothing, so decide it here — locally, in milliseconds.
#
# Exit 0 to run. Any other code skips the run, and whatever this prints becomes
# the reason shown in the UI.
#
#   NOTEMD_NOTE   the one sidecar note to answer, empty for a whole-vault pass
#   NOTEMD_VAULT  the vault root

if [ -n "$NOTEMD_NOTE" ]; then
  [ -f "$NOTEMD_NOTE" ] || { echo "手记文件不存在:$NOTEMD_NOTE"; exit 1; }
  grep -q 'type:: question' "$NOTEMD_NOTE" || { echo "这篇手记里没有问题"; exit 1; }
  grep -q 'status:: open' "$NOTEMD_NOTE" || { echo "这篇手记里没有待答的问题"; exit 1; }
  exit 0
fi

# Whole-vault pass: any open question anywhere?
if [ -n "$NOTEMD_VAULT" ] && grep -rlq --include='*.note.md' 'status:: open' "$NOTEMD_VAULT" 2>/dev/null; then
  exit 0
fi
echo "vault 里没有待答的问题"
exit 1
