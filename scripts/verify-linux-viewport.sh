#!/usr/bin/env bash
# Exercise the native package in an owned, isolated display/session.
set -euo pipefail
if [[ $# -ne 3 ]]; then
  echo "usage: $0 <AppImage|deb> <x11|wayland> <new-evidence-directory>" >&2
  exit 2
fi
artifact="$(realpath "$1")"
backend="$2"
evidence="$(realpath -m "$3")"
[[ "$backend" == x11 || "$backend" == wayland ]]
[[ -f "$artifact" && ! -e "$evidence" ]]
mkdir -p "$evidence"
work="$(mktemp -d "${RUNNER_TEMP:-/tmp}/nbcad-package-display.XXXXXX")"
weston_pid=''
cleanup() {
  if [[ -n "$weston_pid" ]]; then
    kill "$weston_pid" 2>/dev/null || true
    wait "$weston_pid" 2>/dev/null || true
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT
case "$artifact" in
  *.deb)
    dpkg-deb --extract "$artifact" "$work/deb"
    server="$work/deb/usr/bin/nbcad"
    desktop="$work/deb/usr/share/applications/nbcad.desktop"
    ;;
  *.AppImage)
    chmod +x "$artifact"
    (cd "$work" && "$artifact" --appimage-extract >"$evidence/extract.log")
    server="$work/squashfs-root/AppRun"
    desktop="$work/squashfs-root/nbcad.desktop"
    export APPIMAGE="$artifact" APPDIR="$work/squashfs-root"
    ;;
  *) exit 2 ;;
esac
desktop-file-validate "$desktop"
grep -Eq '^MimeType=([^[:space:]]*;)?x-scheme-handler/nbcad(;|$)' "$desktop"
cp "$desktop" "$evidence/packaged.desktop"
mkdir -p "$work/runtime" "$work/config" "$work/data"
chmod 700 "$work/runtime"
export XDG_RUNTIME_DIR="$work/runtime" XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data"
export NBCAD_CONFIG_DIR="$work/config" WGPU_BACKEND=vulkan LIBGL_ALWAYS_SOFTWARE=1
export VK_ICD_FILENAMES="$(find /usr/share/vulkan/icd.d -maxdepth 1 -name 'lvp_icd*.json' -print -quit)"
[[ -n "$VK_ICD_FILENAMES" ]]
if [[ "$backend" == x11 ]]; then
  env -u WAYLAND_DISPLAY dbus-run-session -- xvfb-run -a -s '-screen 0 2560x1600x24' \
    cargo xtask test-mcp native-platform --desktop-input --server "$server" --out "$evidence/native-platform" \
    >"$evidence/fixture.log" 2>&1
else
  weston --backend=headless --renderer=pixman --width=1440 --height=900 \
    --socket=nbcad-package --idle-time=0 >"$evidence/weston.log" 2>&1 &
  weston_pid=$!
  for _ in $(seq 1 100); do
    [[ -S "$XDG_RUNTIME_DIR/nbcad-package" ]] && break
    kill -0 "$weston_pid"
    sleep 0.1
  done
  [[ -S "$XDG_RUNTIME_DIR/nbcad-package" ]]
  # Wayland has no XTEST keyboard route. Verify its actual window through the
  # existing desktop lifecycle/MCP checks, without pretending to send OS keys.
  env -u DISPLAY WAYLAND_DISPLAY=nbcad-package dbus-run-session -- \
    cargo xtask verify-package-mcp --server "$server" --server-arg --headless --desktop \
      --out "$evidence/native-wayland.json" >"$evidence/fixture.log" 2>&1
fi
# The Wayland lifecycle fixture further isolates its child in a private native
# profile. Query that exact child's association, not the outer display harness.
cargo xtask verify-linux-recipe-handler \
  --evidence "$evidence" --server "$server" --artifact "$artifact" --backend "$backend"
