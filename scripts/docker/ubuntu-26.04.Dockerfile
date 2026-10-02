FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH=/root/.cargo/bin:${PATH}

# Official Ubuntu 26.04 build/runtime SDK for the native Bevy/wgpu desktop, HID input and Ubuntu's OpenCASCADE 7.9 packages.
# Ubuntu's data-exchange -dev meta-package also depends on the VTK/IVTK
# development stack. noBS CAD needs its STEP headers, but not those
# visualization SDKs, so the RUN command extracts only that header package
# after installing its runtime and lower-level development dependencies.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        build-essential \
        ca-certificates \
        clang \
        cmake \
        curl \
        dbus-x11 \
        desktop-file-utils \
        file \
        libdbus-1-3 \
        libfuse2t64 \
        libocct-data-exchange-7.9 \
        libocct-foundation-dev \
        libocct-modeling-algorithms-dev \
        libocct-modeling-data-dev \
        libssl-dev \
        libudev-dev \
        libvulkan-dev \
        libwayland-dev \
        libx11-dev \
        libx11-xcb1 \
        libxcursor1 \
        libxi6 \
        libxkbcommon-dev \
        libxkbcommon-x11-dev \
        mesa-vulkan-drivers \
        ninja-build \
        patchelf \
        squashfs-tools \
        pkg-config \
        vulkan-tools \
        weston \
        wget \
        xauth \
        xdg-utils \
        xdg-desktop-portal \
        xdg-desktop-portal-gtk \
        xvfb \
        zenity \
    && if apt-cache show xwayland >/dev/null 2>&1; then \
         apt-get install --yes --no-install-recommends xwayland; \
       fi \
    && cd /tmp \
    && apt-get download libocct-data-exchange-dev \
    && dpkg-deb --extract libocct-data-exchange-dev_*.deb / \
    && rm -f libocct-data-exchange-dev_*.deb \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /workspace
