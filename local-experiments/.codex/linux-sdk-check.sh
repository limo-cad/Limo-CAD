set -euo pipefail
sudo apt-get update
sudo apt-get install --yes --no-install-recommends \
  build-essential ca-certificates clang cmake dbus-x11 desktop-file-utils file \
  libayatana-appindicator3-dev libfuse2t64 libgtk-3-dev \
  libocct-data-exchange-7.9 libocct-foundation-dev \
  libocct-modeling-algorithms-dev libocct-modeling-data-dev \
  librsvg2-dev libssl-dev libudev-dev libvulkan-dev libwayland-dev \
  libwebkit2gtk-4.1-dev libx11-dev libxdo-dev libxkbcommon-dev libxkbcommon-x11-dev \
  mesa-vulkan-drivers ninja-build patchelf pkg-config vulkan-tools weston \
  xauth xdg-utils xwayland xvfb
if [[ "$DESKTOP_INPUT" == true ]]; then
  sudo apt-get install --yes --no-install-recommends \
    fonts-noto-cjk fonts-noto-core fonts-noto-color-emoji \
    ibus ibus-libpinyin imagemagick xclip xdotool
fi
# Ubuntu's STEP -dev package pulls the unused VTK/IVTK SDK. Install
# matching runtime/lower SDKs normally and extract only STEP headers.
sdk_headers="$(mktemp -d "$RUNNER_TEMP/nbcad-occt-headers.XXXXXX")"
(
  cd "$sdk_headers"
  apt-get download libocct-data-exchange-dev
  package="$(find . -maxdepth 1 -name 'libocct-data-exchange-dev_*.deb' -print -quit)"
  test -n "$package"
  sudo dpkg-deb --extract "$package" /
)
