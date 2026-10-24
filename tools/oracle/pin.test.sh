#!/usr/bin/env bash
# Regressions for the platform-keyed oracle-pin.txt reader.
set -euo pipefail

dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=pin.sh
source "$dir/pin.sh"
pin=$dir/oracle-pin.txt

[[ $(oracle_platform x86_64) == linux/amd64 ]]
[[ $(oracle_platform aarch64) == linux/arm64 ]]
[[ $(oracle_platform arm64) == linux/arm64 ]]
if oracle_platform riscv64 >/dev/null; then
	echo 'FAIL: riscv64 was mapped to a frozen platform' >&2
	exit 1
fi

amd64=$(pin_platform_field "$pin" linux/amd64 libpinyin_sha256_bdb)
arm64=$(pin_platform_field "$pin" linux/arm64 libpinyin_sha256_bdb)
[[ $amd64 =~ ^[0-9a-f]{64}$ && $arm64 =~ ^[0-9a-f]{64}$ ]]
[[ $amd64 != "$arm64" ]]
[[ $(pin_platform_field "$pin" linux/arm64 platform) == linux/arm64 ]]
[[ -z $(pin_platform_field "$pin" linux/riscv64 libpinyin_sha256_bdb) ]]

for dbm in bdb kc tkrzw; do
	for lib in libpinyin libzhuyin; do
		[[ $(pin_platform_field "$pin" linux/arm64 "${lib}_sha256_${dbm}") =~ ^[0-9a-f]{64}$ ]]
	done
	[[ $(pin_platform_field "$pin" linux/arm64 "data_manifest_sha256_${dbm}") =~ ^[0-9a-f]{64}$ ]]
done
printf 'PASS: platform-keyed oracle pin selection\n'
