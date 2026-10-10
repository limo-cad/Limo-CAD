#!/usr/bin/env bash
set -euo pipefail
if [[ $# -ne 2 ]]; then
  echo "usage: $0 <native.deb|AppImage> <new-evidence-directory>" >&2
  exit 2
fi
artifact="$(realpath "$1")"
evidence="$(realpath -m "$2")"
[[ -f "$artifact" && ! -e "$evidence" ]]
mkdir -p "$evidence"
work="$(mktemp -d "${RUNNER_TEMP:-/tmp}/limo-cad-native-package.XXXXXX")"
cleanup() { rm -rf -- "$work"; }
trap cleanup EXIT
case "$artifact" in
  *.deb)
    dpkg-deb --extract "$artifact" "$work/deb"
    server="$work/deb/usr/bin/limo-cad"
    ;;
  *.AppImage)
    chmod +x "$artifact"
    (cd "$work" && "$artifact" --appimage-extract >"$evidence/extract.log")
    server="$work/squashfs-root/AppRun"
    ;;
  *) echo 'Expected a DEB or AppImage package' >&2; exit 2 ;;
esac
[[ -x "$server" ]]
vulkan_icd="$(find /usr/share/vulkan/icd.d -maxdepth 1 -type f -name 'lvp_icd*.json' -print -quit)"
[[ -n "$vulkan_icd" ]]
for scale in 1 2; do
  profile="$evidence/scale-$scale"
  mkdir -p "$profile/config" "$profile/cache" "$profile/data" "$profile/runtime"
  chmod 700 "$profile/runtime"
  env -u WAYLAND_DISPLAY -u LD_LIBRARY_PATH -u LD_PRELOAD -u LD_AUDIT \
    LIMO_CAD_CONFIG_DIR="$profile/config" XDG_CONFIG_HOME="$profile/config" \
    XDG_CACHE_HOME="$profile/cache" XDG_DATA_HOME="$profile/data" \
    XDG_RUNTIME_DIR="$profile/runtime" WINIT_X11_SCALE_FACTOR="$scale" \
    WGPU_BACKEND=vulkan VK_ICD_FILENAMES="$vulkan_icd" LIBGL_ALWAYS_SOFTWARE=1 \
    dbus-run-session -- xvfb-run --auto-servernum --server-args='-screen 0 2560x1600x24' \
    cargo xtask test-mcp native-platform --desktop-input \
      --server "$server" --out "$profile/evidence" 2>&1 | tee "$profile/fixture.log"
done
