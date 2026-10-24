#!/usr/bin/env bash
# Platform-keyed reads of tools/oracle/oracle-pin.txt.
#
# The historical linux/amd64 hash fields keep their unqualified names. Other
# platforms add a suffix made from the platform with '/' replaced by '_':
# linux/arm64 reads <field>_linux_arm64. Shared recipe fields (base image,
# snapshot, pin commit, model digest) remain unqualified.

oracle_platform() {
	case ${1:-$(uname -m)} in
	x86_64 | amd64) printf '%s\n' linux/amd64 ;;
	aarch64 | arm64) printf '%s\n' linux/arm64 ;;
	*) return 1 ;;
	esac
}

oracle_platform_suffix() {
	case $1 in
	linux/amd64) ;;
	linux/arm64) printf '%s\n' _linux_arm64 ;;
	*) return 1 ;;
	esac
}

# pin_platform_field PIN PLATFORM KEY: print a platform-specific field value.
pin_platform_field() {
	local suffix
	suffix=$(oracle_platform_suffix "$2") || return 1
	sed -n "s/^$3${suffix}=//p" "$1"
}
