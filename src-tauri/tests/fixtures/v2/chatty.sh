#!/bin/sh
# command.execute is never answered, but the plugin keeps talking: a background
# subshell pushes a host.ui.post every 200ms for ~2s and then goes quiet.
# Pins the "silence timeout, not elapsed-time timeout" contract: the request must
# survive far past request_timeout while the chatter lasts, and only fail once
# the process has actually gone silent for that long.
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"$initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"ok":true}}\n' "$id" ;;
    *'"$activate"'*)   printf '{"jsonrpc":"2.0","id":%s,"result":{"ok":true}}\n' "$id" ;;
    *'"command.execute"'*)
      (
        i=0
        while [ $i -lt 10 ]; do
          printf '{"jsonrpc":"2.0","method":"host.ui.post","params":{"window_id":"main","payload":{"tick":%s}}}\n' "$i"
          sleep 0.2
          i=$((i + 1))
        done
      ) &
      ;;
    *'"$deactivate"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"ok":true}}\n' "$id"; exit 0 ;;
  esac
done
