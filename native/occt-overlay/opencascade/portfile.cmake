string(REPLACE "." "_" VERSION_STR "V${VERSION}")
vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO Open-Cascade-SAS/OCCT
    REF "${VERSION_STR}"
    SHA512 65935a2f46021e2b9a7dd2a218515c06925454855a8cc952fcbd1cccbfd5c8d605ed8f1d930d2aec87ef172ded551d0a237fad128319fb9cbcabdc755aa0aa67
    HEAD_REF master
    PATCHES
        fix-install-prefix-path.patch
        drop-bin-letter-d.patch
        dependencies.patch
        install-include-dir.patch
        remove-vcpkg-enabling.patch
)

foreach(math_source math_DoubleTab.cxx math_DoubleTab.lxx)
    file(SHA256 "${SOURCE_PATH}/src/math/${math_source}" original)
    if(math_source STREQUAL "math_DoubleTab.cxx")
        set(expected "bc046ea106812a0f2c37d47a5f54cd58099837dcb6a68c1c86749db4077d5b43")
    else()
        set(expected "2cf1b4d1c9f855e04b00edaae56d5fb700769db50a3d7000341fd3c269116b54")
    endif()
    if(NOT original STREQUAL expected)
        message(FATAL_ERROR "Checked OCCT storage override input differs: ${math_source}")
    endif()
    configure_file("${CMAKE_CURRENT_LIST_DIR}/${math_source}" "${SOURCE_PATH}/src/math/${math_source}" COPYONLY)
endforeach()

if (VCPKG_LIBRARY_LINKAGE STREQUAL "dynamic")
    set(BUILD_TYPE "Shared")
else()
    set(BUILD_TYPE "Static")
endif()

vcpkg_check_features(OUT_FEATURE_OPTIONS FEATURE_OPTIONS
    FEATURES
        freeimage   USE_FREEIMAGE
        freetype    USE_FREETYPE
        rapidjson   USE_RAPIDJSON
        samples     INSTALL_SAMPLES
        tbb         USE_TBB
        vtk         USE_VTK
)

vcpkg_cmake_configure(
    SOURCE_PATH "${SOURCE_PATH}"
    OPTIONS
        ${FEATURE_OPTIONS}
        -DBUILD_LIBRARY_TYPE=${BUILD_TYPE}
        -DBUILD_MODULE_Draw=OFF
        -DBUILD_DOC_Overview=OFF
        -DBUILD_MODULE_DETools=OFF
        -DINSTALL_DIR_LAYOUT=Unix
        -DINSTALL_DIR_DOC=share/trash
        -DINSTALL_DIR_SCRIPT=share/trash
        -DINSTALL_TEST_CASES=OFF
        -DUSE_TK=OFF
    OPTIONS_DEBUG
        -DINSTALL_SAMPLES=OFF
)

vcpkg_cmake_install()
vcpkg_copy_pdbs()
vcpkg_cmake_config_fixup(CONFIG_PATH lib/cmake/opencascade)

file(GLOB extra_headers
    LIST_DIRECTORIES false
    RELATIVE "${CURRENT_PACKAGES_DIR}/include/opencascade"
    "${CURRENT_PACKAGES_DIR}/include/opencascade/*.h"
)
list(JOIN extra_headers "|" extra_headers)
file(GLOB files "${CURRENT_PACKAGES_DIR}/include/opencascade/*.[hgl]xx")
foreach(file_name IN LISTS files)
    vcpkg_replace_string("${file_name}" "(# *include) <([a-zA-Z0-9_]*[.][hgl]xx|${extra_headers})>" [[\1 "\2"]] REGEX IGNORE_UNCHANGED)
endforeach()

if(VCPKG_LIBRARY_LINKAGE STREQUAL "static")
    vcpkg_replace_string("${CURRENT_PACKAGES_DIR}/include/opencascade/Standard_Macro.hxx" "defined(OCCT_STATIC_BUILD)" "(1)")
endif()

file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/debug/include")
file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/debug/share")
file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/share/opencascade/samples/qt")
file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/share/trash")

vcpkg_install_copyright(
    FILE_LIST
        "${SOURCE_PATH}/LICENSE_LGPL_21.txt"
        "${SOURCE_PATH}/OCCT_LGPL_EXCEPTION.txt"
)
