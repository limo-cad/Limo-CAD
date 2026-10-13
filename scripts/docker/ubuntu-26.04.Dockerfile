# syntax=docker/dockerfile:1
FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH=/root/.cargo/bin:${PATH}
ENV OCCT_ROOT=/opt/opencascade
ENV LD_LIBRARY_PATH=/opt/opencascade/lib

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
        git \
        libdbus-1-3 \
        libfontconfig-dev \
        libfuse2t64 \
        libfreetype6-dev \
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
    && rm -rf /var/lib/apt/lists/*

COPY rust-toolchain.toml /opt/limo-cad-toolchain/rust-toolchain.toml
WORKDIR /opt/limo-cad-toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain none
RUN rustup show

COPY Cargo.toml Cargo.lock rust-toolchain.toml VERSION /tmp/limo-cad-build-tools/
COPY .cargo/config.toml .cargo/tools.toml /tmp/limo-cad-build-tools/.cargo/
WORKDIR /tmp/limo-cad-build-tools
RUN mkdir -p crates xtask assets/i18n native
ARG CMAKE_BUILD_PARALLEL_LEVEL
RUN --mount=type=bind,source=crates,target=/tmp/limo-cad-build-tools/crates \
    --mount=type=bind,source=xtask,target=/tmp/limo-cad-build-tools/xtask \
    --mount=type=bind,source=native,target=/tmp/limo-cad-build-tools/native \
    --mount=type=bind,source=assets/i18n,target=/tmp/limo-cad-build-tools/assets/i18n \
    --mount=type=cache,target=/var/cache/limo-cad-rust-target,sharing=locked \
    --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    --mount=type=cache,target=/var/cache/limo-cad-sdk,sharing=locked \
    CARGO_TARGET_DIR=/var/cache/limo-cad-rust-target LIMO_CAD_BUILD_CACHE=/var/cache/limo-cad-sdk \
      cargo run --quiet --locked -p xtask -- build-occt --prefix /opt/opencascade
RUN rm -rf /tmp/limo-cad-build-tools

WORKDIR /workspace
