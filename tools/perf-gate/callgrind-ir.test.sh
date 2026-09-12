#!/usr/bin/env bash
# callgrind-ir.test.sh — fixture tests for tools/perf-gate/callgrind-ir.py.
#
# Each fixture isolates one property of the callgrind format that the first
# version of this parser got wrong. The expected numbers are hand-computed from
# the fixture text, which is the point: they are checkable by reading.
#
# Run: tools/perf-gate/callgrind-ir.test.sh

set -euo pipefail
cd "$(dirname "$0")"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
pass=0
fail=0

check() { # <label> <fixture-file> <object-substring> <expected>
	local label=$1 file=$2 want_ob=$3 want=$4 got
	got=$(./callgrind-ir.py "$file" "$want_ob")
	if [ "$got" = "$want" ]; then
		printf 'ok   %-50s %s\n' "$label" "$got"
		pass=$((pass + 1))
	else
		printf 'FAIL %-50s want %s got %s\n' "$label" "$want" "$got"
		fail=$((fail + 1))
	fi
}

# --- self cost only, no compression ----------------------------------------
cat > "$WORK/plain.out" <<'EOF'
positions: line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 100
11 200
EOF
check "plain self costs sum" "$WORK/plain.out" libpinyin 300

# --- a different object must not contribute --------------------------------
cat > "$WORK/two-obs.out" <<'EOF'
positions: line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 100
ob=/lib/libc.so.6
fl=/src/c.c
fn=memcpy
20 9999
EOF
check "another object is excluded" "$WORK/two-obs.out" libpinyin 100

# --- name compression: the bare back-reference must still count -------------
# The first version tested the line text for "libpinyin", so it counted the
# 100 under the definition and silently dropped the 250 under `ob=(1)`.
cat > "$WORK/compressed.out" <<'EOF'
positions: line
events: Ir
ob=(1) /usr/lib/libpinyin.so.15
fl=(1) /src/a.c
fn=(1) alpha
10 100
ob=(2) /lib/libc.so.6
fl=(2) /src/c.c
fn=(2) memcpy
20 9999
ob=(1)
fl=(1)
fn=(3) beta
30 250
EOF
check "compressed ob back-reference counts" "$WORK/compressed.out" libpinyin 350

# --- call cost lines are inclusive and must be skipped ----------------------
# Naive summing adds the 5000 inclusive cost and reports 5150.
cat > "$WORK/calls.out" <<'EOF'
positions: line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 100
cfn=callee
calls=3 42
10 5000
11 50
EOF
check "call-inclusive cost is skipped" "$WORK/calls.out" libpinyin 150

# --- Ir is selected by name, not by position -------------------------------
# With Ir second, a fixed first-column reader returns the Dr values (7+9=16).
cat > "$WORK/events.out" <<'EOF'
positions: line
events: Dr Ir Dw
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 7 100 3
11 9 200 4
EOF
check "Ir column chosen from events:" "$WORK/events.out" libpinyin 300

# --- two position columns under --dump-instr=yes ---------------------------
cat > "$WORK/instr.out" <<'EOF'
positions: instr line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
0x1000 10 100
0x1004 11 200
EOF
check "two position columns" "$WORK/instr.out" libpinyin 300

# --- cob= names a callee's object and must not retarget self costs ---------
cat > "$WORK/cob.out" <<'EOF'
positions: line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 100
cob=/lib/libc.so.6
cfn=memcpy
calls=1 0
10 4000
11 25
EOF
check "cob= does not retarget self cost" "$WORK/cob.out" libpinyin 125

# --- nothing attributed, and no Ir event -----------------------------------
cat > "$WORK/nomatch.out" <<'EOF'
positions: line
events: Ir
ob=/lib/libc.so.6
fl=/src/c.c
fn=memcpy
20 42
EOF
check "no records for the object is null" "$WORK/nomatch.out" libpinyin null

cat > "$WORK/noir.out" <<'EOF'
positions: line
events: Dr Dw
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 7 3
EOF
check "a file with no Ir event is null" "$WORK/noir.out" libpinyin null

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
