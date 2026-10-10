#!/usr/bin/env bash
set -euo pipefail
if [[ $# -ne 3 ]]; then
  echo "usage: $0 <AppImage|deb> <x11|wayland> <new-evidence-directory>" >&2
  exit 2
fi
artifact="$(realpath "$1")"
backend="$2"
destination="$(realpath -m "$3")"
[[ "$backend" == x11 || "$backend" == wayland ]]
[[ -f "$artifact" && ! -e "$destination" ]]
mkdir -p "$destination"
work="$(mktemp -d /tmp/limo-cad-package-display.XXXXXX)"
evidence="$work/evidence"
mkdir "$evidence"
weston_pid=''
cleanup() {
  if [[ -n "$weston_pid" ]]; then
    kill "$weston_pid" 2>/dev/null || true
    wait "$weston_pid" 2>/dev/null || true
  fi
  cp -a "$evidence/." "$destination/"
  [[ "$work" == /tmp/limo-cad-package-display.* ]]
  rm -rf -- "$work"
}
trap cleanup EXIT
case "$artifact" in
  *.deb)
    dpkg-deb --extract "$artifact" "$work/deb"
    server="$work/deb/usr/bin/limo-cad"
    desktop="$work/deb/usr/share/applications/limo-cad.desktop"
    ;;
  *.AppImage)
    chmod +x "$artifact"
    (cd "$work" && "$artifact" --appimage-extract >"$evidence/extract.log")
    server="$work/squashfs-root/AppRun"
    desktop="$work/squashfs-root/limo-cad.desktop"
    export APPIMAGE="$artifact" APPDIR="$work/squashfs-root"
    ;;
  *) exit 2 ;;
esac
desktop-file-validate "$desktop"
grep -Eq '^MimeType=([^[:space:]]*;)?x-scheme-handler/limo-cad(;|$)' "$desktop"
cp "$desktop" "$evidence/packaged.desktop"
mkdir -p "$work/runtime" "$work/config" "$work/data"
chmod 700 "$work/runtime"
export XDG_RUNTIME_DIR="$work/runtime" XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data"
export LIMO_CAD_CONFIG_DIR="$work/config" WGPU_BACKEND=vulkan LIBGL_ALWAYS_SOFTWARE=1
export VK_ICD_FILENAMES="$(find /usr/share/vulkan/icd.d -maxdepth 1 -name 'lvp_icd*.json' -print -quit)"
[[ -n "$VK_ICD_FILENAMES" ]]
if [[ "$backend" == x11 ]]; then
  env -u WAYLAND_DISPLAY dbus-run-session -- xvfb-run -a -s '-screen 0 2560x1600x24' \
    cargo xtask test-mcp native-platform --desktop-input --server "$server" --out "$evidence/native-platform" \
    >"$evidence/fixture.log" 2>&1
else
  weston --backend=headless --renderer=pixman --width=1440 --height=900 \
    --socket=limo-cad-package --idle-time=0 >"$evidence/weston.log" 2>&1 &
  weston_pid=$!
  for _ in $(seq 1 100); do
    [[ -S "$XDG_RUNTIME_DIR/limo-cad-package" ]] && break
    kill -0 "$weston_pid"
    sleep 0.1
  done
  [[ -S "$XDG_RUNTIME_DIR/limo-cad-package" ]]
  env -u DISPLAY WAYLAND_DISPLAY=limo-cad-package dbus-run-session -- \
    cargo xtask verify-package-mcp --server "$server" --server-arg --headless --desktop \
      --out "$evidence/native-wayland.json" >"$evidence/fixture.log" 2>&1
fi
cargo xtask verify-linux-recipe-handler \
  --evidence "$evidence" --server "$server" --artifact "$artifact" --backend "$backend"
