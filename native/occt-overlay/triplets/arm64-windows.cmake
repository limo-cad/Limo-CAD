# Preserve the standard dynamic ARM64 ABI while avoiding the hosted MSVC 14.52
# optimizer crash: https://github.com/actions/runner-images/issues/15328
set(VCPKG_TARGET_ARCHITECTURE arm64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE dynamic)
set(VCPKG_PLATFORM_TOOLSET v143)
file(STRINGS "${CMAKE_CURRENT_LIST_DIR}/msvc-arm64-version.txt"
    VCPKG_PLATFORM_TOOLSET_VERSION LIMIT_COUNT 1)
set(VCPKG_HASH_ADDITIONAL_FILES "${CMAKE_CURRENT_LIST_DIR}/msvc-arm64-version.txt")
