# Keep shared SDK producers and consumers on the same installed LTS compiler
# while hosted Windows runner images roll between different default toolsets.
set(VCPKG_TARGET_ARCHITECTURE x64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE dynamic)
set(VCPKG_PLATFORM_TOOLSET v143)
file(STRINGS "${CMAKE_CURRENT_LIST_DIR}/msvc-x64-version.txt"
    VCPKG_PLATFORM_TOOLSET_VERSION LIMIT_COUNT 1)
set(VCPKG_HASH_ADDITIONAL_FILES "${CMAKE_CURRENT_LIST_DIR}/msvc-x64-version.txt")
