cmake_minimum_required(VERSION 3.20)
project(tkrzw LANGUAGES C CXX)

# Use the pin's library inventory, not its tests or command-line programs.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/VCMakefile" upstream)
string(REGEX MATCH "PKG_VERSION = ([^\r\n]+)" unused "${upstream}")
if(NOT CMAKE_MATCH_1 STREQUAL TKRZW_VERSION)
    message(FATAL_ERROR "Upstream version differs from the shared pin")
endif()
string(REGEX MATCH "LIB_VERSION = ([^\r\n]+)" unused "${upstream}")
set(library_version "${CMAKE_MATCH_1}")
string(REGEX MATCH "LIBOBJFILES = ([^\r\n]+)" unused "${upstream}")
string(REPLACE ".obj" ".cc" sources "${CMAKE_MATCH_1}")
separate_arguments(sources)
string(REGEX MATCH "HEADERFILES = ([^\r\n]+)" unused "${upstream}")
separate_arguments(headers NATIVE_COMMAND "${CMAKE_MATCH_1}")

add_library(tkrzw ${sources})
target_compile_features(tkrzw PRIVATE cxx_std_17)
target_compile_options(tkrzw PRIVATE /Zc:__cplusplus /EHsc)
if(BUILD_SHARED_LIBS)
    target_compile_definitions(tkrzw PRIVATE TKRZW_SHARED TKRZW_BUILD_DLL)
endif()
target_compile_definitions(tkrzw PRIVATE _CRT_SECURE_NO_WARNINGS
    "_TKRZW_PKG_VERSION=\"${TKRZW_VERSION}\""
    "_TKRZW_LIB_VERSION=\"${library_version}\"")
# Optional compression matches upstream's configure switches, default off.
option(TKRZW_WITH_ZLIB "Enable zlib compression" OFF)
option(TKRZW_WITH_ZSTD "Enable Zstandard compression" OFF)
option(TKRZW_WITH_LZ4 "Enable LZ4 compression" OFF)
option(TKRZW_WITH_LZMA "Enable LZMA compression" OFF)
set(pc_requires "")
if(TKRZW_WITH_ZLIB)
    find_package(ZLIB REQUIRED)
    target_link_libraries(tkrzw PRIVATE ZLIB::ZLIB)
    target_compile_definitions(tkrzw PRIVATE _TKRZW_COMP_ZLIB=1)
    string(APPEND pc_requires " zlib")
endif()
if(TKRZW_WITH_ZSTD)
    find_package(zstd CONFIG REQUIRED)
    target_link_libraries(tkrzw PRIVATE zstd::libzstd)
    target_compile_definitions(tkrzw PRIVATE _TKRZW_COMP_ZSTD=1)
    string(APPEND pc_requires " libzstd")
endif()
if(TKRZW_WITH_LZ4)
    find_package(lz4 CONFIG REQUIRED)
    target_link_libraries(tkrzw PRIVATE lz4::lz4)
    target_compile_definitions(tkrzw PRIVATE _TKRZW_COMP_LZ4=1)
    string(APPEND pc_requires " liblz4")
endif()
if(TKRZW_WITH_LZMA)
    find_package(LibLZMA REQUIRED)
    target_link_libraries(tkrzw PRIVATE LibLZMA::LibLZMA)
    target_compile_definitions(tkrzw PRIVATE _TKRZW_COMP_LZMA=1)
    string(APPEND pc_requires " liblzma")
endif()
install(TARGETS tkrzw RUNTIME DESTINATION bin ARCHIVE DESTINATION lib)
install(FILES ${headers} DESTINATION include)

# A whole-list variable makes pkgconf tokenize escaped paths after expansion,
# including pkgconf 3. pcfiledir keeps this metadata relocatable through vcpkg.
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/tkrzw.pc"
    "prefix=\${pcfiledir}/../..\nlibdir=\${prefix}/lib\nincludedir=\${prefix}/include\n"
    "Name: Tkrzw\nDescription: Thread-safe DBM library\nVersion: ${TKRZW_VERSION}\n"
    "tkrzw_libs=-L\${libdir} -ltkrzw\ntkrzw_cflags=-I\${includedir}\n"
    "Libs: \${tkrzw_libs}\nCflags: \${tkrzw_cflags}\nRequires.private:${pc_requires}\n")
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/tkrzw.pc" DESTINATION lib/pkgconfig)
