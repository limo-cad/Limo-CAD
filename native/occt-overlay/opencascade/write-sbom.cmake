# Keep OCCT build evidence inside the vcpkg binary cache entry. The port verifies
# the upstream SHA512 and original math header before compiling this variant.
set(sbom "{\n  \"schemaVersion\": 1,\n  \"version\": \"${VERSION}\",\n  \"triplet\": \"${TARGET_TRIPLET}\",\n  \"sourceSha512\": \"bbe7099071cbf5397940ebc6e66ec05f8023d5e5aae6142870e14b93aa6f8f94c30980ef421e717f0fbfbc23b3520c3ccfe8a939c4caba3ccbf325060e26eb52\",\n  \"patches\": {\n")
set(separator "")
foreach(patch 0001-cmake-keep-build-use-vcpkg-explicit.patch
    0002-cmake-load-exported-package-dependencies.patch
    0003-image-remove-freeimage-msvc-autolink.patch
    0004-cmake-add-additional-path-extraction-for-OpenCASCADE.patch
    0005-drop-bin-letter.patch
    math_DoubleTab.hxx)
    file(SHA256 "${CMAKE_CURRENT_LIST_DIR}/${patch}" digest)
    string(APPEND sbom "${separator}    \"${patch}\": \"${digest}\"")
    set(separator ",\n")
endforeach()
string(APPEND sbom "\n  },\n  \"files\": {\n")
set(separator "")
file(GLOB libraries RELATIVE "${CURRENT_PACKAGES_DIR}" "${CURRENT_PACKAGES_DIR}/bin/TK*.dll")
list(SORT libraries)
foreach(library IN LISTS libraries)
    file(SHA256 "${CURRENT_PACKAGES_DIR}/${library}" digest)
    string(APPEND sbom "${separator}    \"${library}\": \"${digest}\"")
    set(separator ",\n")
endforeach()
string(APPEND sbom "\n  }\n}\n")
file(WRITE "${CURRENT_PACKAGES_DIR}/share/opencascade/limo-occt-build.json" "${sbom}")
