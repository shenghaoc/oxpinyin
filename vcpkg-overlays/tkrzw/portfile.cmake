file(READ "${CMAKE_CURRENT_LIST_DIR}/pin.json" pin)
string(JSON revision GET "${pin}" revision)
string(JSON source_hash GET "${pin}" sha512)
string(JSON pinned_version GET "${pin}" version)
if(NOT VERSION STREQUAL pinned_version)
    message(FATAL_ERROR "tkrzw port version differs from the shared pin")
endif()

vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO estraier/tkrzw
    REF "${revision}"
    SHA512 "${source_hash}"
    PATCHES
        dll-export.patch
        # --disable-shared drops the macro that declares F_NOCACHE on macOS.
        darwin-static.patch
)
if(VCPKG_TARGET_IS_WINDOWS)
    file(COPY "${CMAKE_CURRENT_LIST_DIR}/CMakeLists.txt" DESTINATION "${SOURCE_PATH}")
    vcpkg_check_features(OUT_FEATURE_OPTIONS feature_options
        FEATURES
            zlib TKRZW_WITH_ZLIB
            zstd TKRZW_WITH_ZSTD
            lz4 TKRZW_WITH_LZ4
            lzma TKRZW_WITH_LZMA
    )
    vcpkg_cmake_configure(SOURCE_PATH "${SOURCE_PATH}"
        OPTIONS "-DTKRZW_VERSION=${pinned_version}" ${feature_options})
    vcpkg_cmake_install()
else()
    # Upstream's own configure and Makefile, as Homebrew runs them. The
    # configure script is committed, so nothing regenerates it, and the
    # Makefile finds its sources only from the source directory.
    set(linkage_options "")
    if(VCPKG_LIBRARY_LINKAGE STREQUAL "static")
        set(linkage_options --disable-shared)
    endif()
    vcpkg_make_configure(SOURCE_PATH "${SOURCE_PATH}" COPY_SOURCE
        # Upstream's --enable-static links the utilities with -static.
        DEFAULT_OPTIONS_EXCLUDE "^--(enable|disable)-(shared|static)$"
        OPTIONS ${linkage_options})
    vcpkg_make_install()
    foreach(lib IN ITEMS "${CURRENT_PACKAGES_DIR}/lib" "${CURRENT_PACKAGES_DIR}/debug/lib")
        # Upstream builds libtkrzw.a beside the shared library.
        if(VCPKG_LIBRARY_LINKAGE STREQUAL "dynamic")
            file(REMOVE "${lib}/libtkrzw.a")
        endif()
        # configure records -lstdc++; macOS links libc++ and has no libstdc++.
        if(VCPKG_TARGET_IS_OSX AND EXISTS "${lib}/pkgconfig/tkrzw.pc")
            file(READ "${lib}/pkgconfig/tkrzw.pc" module)
            string(REPLACE "-lstdc++" "-lc++" module "${module}")
            file(WRITE "${lib}/pkgconfig/tkrzw.pc" "${module}")
        endif()
    endforeach()
    file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/debug/share" "${CURRENT_PACKAGES_DIR}/tools/${PORT}/debug")
endif()
# Freeze the installed API's linkage; consumers need no preprocessor flags.
foreach(header IN ITEMS tkrzw_lib_common.h)
    set(path "${CURRENT_PACKAGES_DIR}/include/${header}")
    file(READ "${path}" contents)
    string(REGEX MATCHALL "#if defined\\(_WIN32\\) && defined\\(TKRZW_SHARED\\)" linkage_conditions "${contents}")
    list(LENGTH linkage_conditions linkage_condition_count)
    if(NOT linkage_condition_count EQUAL 1)
        message(FATAL_ERROR "Expected exactly one DLL linkage condition in ${header}; found ${linkage_condition_count}")
    endif()
    if(VCPKG_LIBRARY_LINKAGE STREQUAL "static")
        set(condition "#if 0")
    else()
        set(condition "#if defined(_WIN32)")
    endif()
    string(REPLACE "#if defined(_WIN32) && defined(TKRZW_SHARED)" "${condition}" contents "${contents}")
    file(WRITE "${path}" "${contents}")
endforeach()
vcpkg_copy_pdbs()
vcpkg_fixup_pkgconfig()
file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/debug/include")
vcpkg_install_copyright(FILE_LIST "${SOURCE_PATH}/COPYING")
file(INSTALL "${CMAKE_CURRENT_LIST_DIR}/usage" DESTINATION "${CURRENT_PACKAGES_DIR}/share/${PORT}")
