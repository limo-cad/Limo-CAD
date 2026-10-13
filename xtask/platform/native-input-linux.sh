#!/usr/bin/env bash
set -euo pipefail
owned_pid="$1"
operation="$2"
for tool in xclip xdotool; do
  if ! command -v "$tool" > /dev/null; then
    echo "Native X11 input helper requires $tool on the test host" >&2
    exit 1
  fi
done
case "$operation" in
  clipboard-read) xclip -selection clipboard -out 2>/dev/null || true; exit 0 ;;
  clipboard-write) exec xclip -selection clipboard -in >/dev/null 2>/dev/null ;;
  script-dialog)
    title="${LIMO_CAD_SCRIPT_DIALOG_TITLE:?}"
    path="$(cat)"
    dialog=""
    for _ in $(seq 1 150); do
      dialog="$(xdotool search --onlyvisible --name "$title" | head -n 1 || true)"
      if [[ -n "$dialog" ]]; then
        break
      fi
      sleep 0.1
    done
    [[ -n "$dialog" ]]
    xdotool windowfocus --sync "$dialog"
    xdotool type --clearmodifiers --delay 12 "$path"
    xdotool key --clearmodifiers Return
    exit 0
    ;;
  drawing-wheel|drawing-pan|drawing-click|drawing-drag|cam-row-drag) python3 "$(dirname "$0")/native-drawing-linux.py" --verify-private-display >/dev/null ;;
esac
mapfile -t windows < <(xdotool search --onlyvisible --pid "$owned_pid")
if [[ ${#windows[@]} != 1 ]]; then
  echo "Expected one visible window owned by PID $owned_pid, got ${#windows[@]}" >&2
  exit 1
fi
window="${windows[0]}"
xdotool windowfocus --sync "$window"
[[ "$(xdotool getwindowfocus)" == "$window" ]]
case "$operation" in
  focus) ;;
  select-all) xdotool key --clearmodifiers ctrl+a ;;
  copy) xdotool key --clearmodifiers ctrl+c ;;
  paste) xdotool key --clearmodifiers ctrl+v ;;
  right) xdotool key --clearmodifiers Right ;;
  home) xdotool key --clearmodifiers Home ;;
  backspace) xdotool key --clearmodifiers BackSpace ;;
  drawing-wheel|drawing-pan|drawing-click|drawing-drag) exec python3 "$(dirname "$0")/native-drawing-linux.py" "$owned_pid" "$operation" "$window" ;;
  cam-row-drag) exec python3 "$(dirname "$0")/native-cam-row-linux.py" "$owned_pid" "$window" ;;
  ime-*)
    [[ "${LIMO_CAD_NATIVE_IME_TEST:-}" == 1 && "${XMODIFIERS:-}" == '@im=ibus' ]]
    case "$operation" in
      ime-enable)
        if ! ibus engine libpinyin; then
          echo "IBus could not enable libpinyin; observed engine: $(ibus engine 2>&1)" >&2
          exit 1
        fi
        observed_engine="$(ibus engine)"
        if [[ "$observed_engine" != libpinyin ]]; then
          echo "Expected IBus libpinyin, got: $observed_engine" >&2
          exit 1
        fi
        ;;
      ime-disable) ibus engine xkb:us::eng ;;
      ime-preedit) xdotool type --clearmodifiers --delay 80 nihao ;;
      ime-commit) xdotool key --clearmodifiers space ;;
      ime-cancel) xdotool key --clearmodifiers Escape ;;
      ime-evidence) exec python3 "$(dirname "$0")/native-ime-linux.py" "$owned_pid" ;;
      *) echo "Unknown IME operation $operation" >&2; exit 1 ;;
    esac
    ;;
  *) echo "Unknown input operation $operation" >&2; exit 1 ;;
esac
