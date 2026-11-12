# vcpkg's arm64-osx-dynamic with two changes for oxpinyin's macOS builds.
set(VCPKG_TARGET_ARCHITECTURE arm64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE dynamic)

set(VCPKG_CMAKE_SYSTEM_NAME Darwin)
set(VCPKG_OSX_ARCHITECTURES arm64)

# Keep each library's install name as its build records it: the absolute path
# in the installed tree. A consumer then loads the library with no rpath,
# which oxpinyin does not add. The installed tree cannot move afterwards.
set(VCPKG_FIXUP_MACHO_RPATH OFF)
# The install name records the vcpkg root, so the root is part of each
# package's ABI: a binary cache never restores another root's library.
set(VCPKG_ENV_PASSTHROUGH VCPKG_ROOT)
# Nothing in oxpinyin links a debug variant.
set(VCPKG_BUILD_TYPE release)
