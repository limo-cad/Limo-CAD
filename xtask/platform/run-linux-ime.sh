#!/usr/bin/env bash
set -euo pipefail
server="$1"
evidence="$2"
[[ -n "${DISPLAY:-}" && -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]]
[[ ! -e "$evidence" ]]
ime_profile="$(mktemp -d "${RUNNER_TEMP:-/tmp}/limo-cad-ime-profile.XXXXXX")"
export XDG_CONFIG_HOME="$ime_profile/config"
export XDG_CACHE_HOME="$ime_profile/cache"
export XDG_DATA_HOME="$ime_profile/data"
export XDG_RUNTIME_DIR="$ime_profile/runtime"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
export XMODIFIERS='@im=ibus'
export GTK_IM_MODULE=ibus
export QT_IM_MODULE=ibus
export LIMO_CAD_NATIVE_IME_TEST=1
export LANG=C.UTF-8
python3 "$(dirname "$0")/native-ime-linux.py" --verify-private-display >/dev/null
dbus-update-activation-environment XDG_CONFIG_HOME XDG_CACHE_HOME XDG_DATA_HOME \
  XDG_RUNTIME_DIR DISPLAY XMODIFIERS GTK_IM_MODULE QT_IM_MODULE LANG
gsettings set org.freedesktop.ibus.general use-system-keyboard-layout true
ibus-daemon --xim --replace >"$ime_profile/ibus.log" 2>&1 &
export LIMO_CAD_NATIVE_IME_DAEMON_PID=$!
trap 'kill "$LIMO_CAD_NATIVE_IME_DAEMON_PID" 2>/dev/null || true' EXIT
for attempt in $(seq 1 100); do
  kill -0 "$LIMO_CAD_NATIVE_IME_DAEMON_PID"
  if ibus engine xkb:us::eng 2>/dev/null; then break; fi
  sleep 0.1
done
[[ "$(ibus engine)" == xkb:us::eng ]]
gsettings set org.freedesktop.ibus.general use-global-engine true
gsettings set org.freedesktop.ibus.general preload-engines "['xkb:us::eng', 'libpinyin']"
set +e
cargo xtask test-mcp native-platform --desktop-input --ime-libpinyin --server "$server" --out "$evidence"
result=$?
set -e
mkdir -p "$evidence"
cp "$ime_profile/ibus.log" "$evidence/ibus.log"
exit "$result"
