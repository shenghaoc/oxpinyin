# debuginfo-neutrality-lib.sh — is a debug-info build the same code?
#
# Sourced by tools/bisection/run-callgrind-differential.sh (stage 2) and
# tools/env-probe/probe-debuginfo-neutrality.sh, so the two cannot drift.
# Provides compare_debuginfo_neutrality <a.so> <b.so>, which prints a
# human-readable report and returns 0 when the two artifacts carry the
# same instruction stream, 1 when they do not.
#
# Why it exists: the profiled artifact is built with debug info and the
# timed one without (CARGO_PROFILE_RELEASE_DEBUG feeds cargo's crate
# metadata hash, hence every mangled symbol name, hence link order), so
# the two .text sections hold the same functions at different addresses.
# Three outcomes, and only the last is fatal:
#
#   (a) identical .text bytes                  — fully neutral
#   (b) same instruction multiset at different addresses
#       — Ir totals are unaffected (the same instructions execute); the
#         SIMULATED cache and branch figures a callgrind --cache-sim run
#         prints are layout-sensitive and must be reported as qualitative
#         context only, never as measurements
#   (c) a different instruction multiset       — different code. Stop.
#
# The multiset comparison keeps the FULL instruction text, not just the
# mnemonic: same mnemonics with different registers or immediates is a
# real codegen difference and must land in case (c). Only two things are
# normalized away, both pure functions of where the linker placed code:
#   - the target of direct control transfers (branch/call/loop)
#   - rip-relative displacements (including their sign)
# Anything else — registers, immediate operands, displacement off a
# register — is compared verbatim.
#
# The relocation count pairs text symbols by a stable identity with the
# crate-disambiguator hash stripped from v0-mangled names (`Cs…_`), so a
# renamed symbol pairs with itself instead of being miscounted. A handful
# of symbols still rename structurally under fat LTO when the hash
# changes (generic instantiations re-attributed to a different
# instantiating crate; anonymous `.923` suffixes shifting); those are
# paired by identical normalized instruction bodies instead, and every
# tier of the accounting is printed so nothing is silently guessed.

# Disassembly as "origsymbol<TAB>normalized-instruction" pairs. The symbol
# name is recovered from objdump's `<name>:` boundary lines and kept in its
# ORIGINAL (unstripped) form: the Cs-hash strip happens on the paired-name
# lists only, and body pairing must not conflate distinct raw names.
#
# The two link-order normalizations (direct branch targets; PC-relative
# address materialization) are spelled for both syntaxes, because the script
# runs on x86-64 and aarch64 images:
#   - x86-64: j*/loop/call (direct only — `*` marks indirect), and
#     `-?0x…(%rip)` displacements.
#   - aarch64: b, b.cond, bl (direct — br/blr take a register and stay
#     verbatim), cbz/cbnz/tbz/tbnz (target follows a register operand), and
#     adr/adrp, whose objdump `<symbol+off>` annotation also embeds the
#     crate-disambiguator hash that the debug level changes.
# On any other instruction, that hash is stripped from the annotation the
# same way the symbol-pairing tier strips it from names; registers,
# immediates and everything semantic stay verbatim.
neutrality_symbol_lines() { # $1 = .so
    objdump -d --no-show-raw-insn "$1" 2>/dev/null \
        | sed -e 's/[[:space:]]#.*$//' \
              -e 's/-\{0,1\}0x[0-9a-f]\{1,\}(%rip)/RIPREL/g' \
        | awk '
              /^[[:space:]]*[0-9a-f]+ <.*>:$/ {
                  line = $0;
                  sub(/^[[:space:]]*[0-9a-f]+ </, "", line);
                  sub(/>:$/, "", line);
                  current = line;
                  next;
              }
              /file format|^Disassembly|^$/ { next }
              {
                  text = $0;
                  sub(/^[[:space:]]*[0-9a-f]+:[[:space:]]*/, "", text);
                  split(text, w, /[[:space:]]+/);
                  if ((w[1] ~ /^(j|loop)/ || w[1] == "call") && w[2] !~ /^\*/) {
                      text = w[1] " TARGET";
                  } else if (w[1] ~ /^b(\.[a-z]+)?$/ || w[1] == "bl") {
                      text = w[1] " TARGET";
                  } else if (w[1] ~ /^(cbz|cbnz|tbz|tbnz)$/) {
                      text = w[1] " " w[2] " TARGET";
                  } else if (w[1] == "adrp" || w[1] == "adr") {
                      text = w[1] " " w[2] " PCREL";
                  } else {
                      gsub(/Cs[0-9A-Za-z]+_/, "CsH_", text);
                  }
                  print current "\t" text;
              }'
}

# One line per text symbol: "origsymbol<TAB>sha256 of its normalized body".
# Bodies are folded onto one line with SUBSEP so the read loop below sees
# exactly one record per symbol.
neutrality_symbol_bodies() { # $1 = .so
    neutrality_symbol_lines "$1" \
        | awk -F'\t' '
              $1 == "" { next }
              { body[$1] = body[$1] $2 SUBSEP }
              END { for (s in body) print s "\t" body[s] }' \
        | while IFS=$'\t' read -r sym body; do
              printf '%s\t%s\n' "$sym" \
                     "$(printf '%s' "$body" | sha256sum | cut -d' ' -f1)"
          done
}

neutrality_normalize_instructions() { # $1 = .so → normalized lines on stdout
    neutrality_symbol_lines "$1" | cut -f2-
}

compare_debuginfo_neutrality() { # $1 $2 = the two .so; returns 0 = same code
    local a=$1 b=$2

    objcopy -O binary --only-section=.text "$a" /tmp/neut-a.text 2>/dev/null
    objcopy -O binary --only-section=.text "$b" /tmp/neut-b.text 2>/dev/null
    local ha hb
    ha=$(sha256sum /tmp/neut-a.text | cut -d' ' -f1)
    hb=$(sha256sum /tmp/neut-b.text | cut -d' ' -f1)
    echo "with debug info:    .text sha256 $ha ($(stat -c%s /tmp/neut-a.text) bytes)"
    echo "without debug info: .text sha256 $hb ($(stat -c%s /tmp/neut-b.text) bytes)"

    if [ "$ha" = "$hb" ]; then
        echo "PASS (a): identical .text — debug info changed nothing else"
        return 0
    fi

    # Full instruction-text multiset: order-independent, so pure reordering
    # cancels and any real selection change (register, immediate, mnemonic)
    # survives into case (c).
    local ia ib
    neutrality_normalize_instructions "$a" | sort | uniq -c | sort -rn > /tmp/neut-a.hist
    neutrality_normalize_instructions "$b" | sort | uniq -c | sort -rn > /tmp/neut-b.hist
    ia=$(awk '{n+=$1} END{print n}' /tmp/neut-a.hist)
    ib=$(awk '{n+=$1} END{print n}' /tmp/neut-b.hist)
    echo "instructions (normalized full text): $ia vs $ib, $(wc -l < /tmp/neut-a.hist) distinct forms"

    if ! diff -q /tmp/neut-a.hist /tmp/neut-b.hist >/dev/null; then
        echo "FAIL (c): instruction multiset differs — this is different code."
        diff /tmp/neut-a.hist /tmp/neut-b.hist | head -20
        echo "Stop and report. Do not profile an artifact whose instruction"
        echo "stream differs from the one the timing pass measures."
        return 1
    fi

    # Symbol accounting in two pairing tiers, printed in full. Tier 1 pairs
    # by name with the v0 crate-disambiguator hash stripped (`Cs…_`→`CsH_`);
    # tier 2 pairs any leftovers by identical normalized instruction body.
    local a_syms b_syms
    nm --defined-only "$a" | awk '$2~/[tT]/{print $3, $1}' \
        | sed 's/Cs[0-9A-Za-z]\{1,\}_/CsH_/g' | sort -u > /tmp/neut-a.syms
    nm --defined-only "$b" | awk '$2~/[tT]/{print $3, $1}' \
        | sed 's/Cs[0-9A-Za-z]\{1,\}_/CsH_/g' | sort -u > /tmp/neut-b.syms

    local a_only b_only n_a_only n_b_only
    a_only=$(comm -23 <(cut -d' ' -f1 /tmp/neut-a.syms) <(cut -d' ' -f1 /tmp/neut-b.syms))
    b_only=$(comm -13 <(cut -d' ' -f1 /tmp/neut-a.syms) <(cut -d' ' -f1 /tmp/neut-b.syms))
    n_a_only=$(printf '%s' "$a_only" | grep -c . || true)
    n_b_only=$(printf '%s' "$b_only" | grep -c . || true)

    local paired moved
    paired=$(join -j1 <(cut -d' ' -f1-2 /tmp/neut-a.syms) \
                      <(cut -d' ' -f1-2 /tmp/neut-b.syms) | wc -l)
    moved=$(join -j1 <(cut -d' ' -f1-2 /tmp/neut-a.syms) \
                     <(cut -d' ' -f1-2 /tmp/neut-b.syms) \
               | awk '$2 != $3 {c++} END {print c+0}')

    local tier2_note="none"
    if [ "$n_a_only" -gt 0 ]; then
        if [ "$n_a_only" -eq "$n_b_only" ]; then
            # Match the leftover multisets by body hash: 1:1 identical-code
            # proof for every leftover, or the shortfall is reported. The
            # bodies carry raw symbol names, so their names go through the
            # same Cs-hash strip the leftover lists already had.
            printf '%s\n' "$a_only" | sort > /tmp/neut-a.left
            printf '%s\n' "$b_only" | sort > /tmp/neut-b.left
            neutrality_symbol_bodies "$a" \
                | awk -F'\t' '{ n = $1; gsub(/Cs[0-9A-Za-z]+_/, "CsH_", n);
                                print n "\t" $2 }' | sort > /tmp/neut-a.bodies
            neutrality_symbol_bodies "$b" \
                | awk -F'\t' '{ n = $1; gsub(/Cs[0-9A-Za-z]+_/, "CsH_", n);
                                print n "\t" $2 }' | sort > /tmp/neut-b.bodies
            awk -F'\t' 'NR==FNR{want[$1]=1;next} want[$1]{print $2}' \
                /tmp/neut-a.left /tmp/neut-a.bodies | sort > /tmp/neut-a.left-hashes
            awk -F'\t' 'NR==FNR{want[$1]=1;next} want[$1]{print $2}' \
                /tmp/neut-b.left /tmp/neut-b.bodies | sort > /tmp/neut-b.left-hashes
            local matched
            matched=$(comm -12 /tmp/neut-a.left-hashes /tmp/neut-b.left-hashes | wc -l)
            if [ "$matched" -eq "$n_a_only" ]; then
                tier2_note="$n_a_only paired by identical instruction body"
            else
                tier2_note="only $matched of $n_a_only paired by instruction body"
            fi
        else
            tier2_note="asymmetric leftovers ($n_a_only vs $n_b_only) — not pairable"
        fi
    fi

    echo "PASS (b): identical instruction multiset (full normalized text)."
    echo "  Symbol accounting: $paired symbols pair by hash-stripped name,"
    echo "  $moved of those at different addresses; unpaired after hash"
    echo "  strip: $n_a_only — $tier2_note."
    echo "  Ir totals are unaffected by relocation. The SIMULATED cache and"
    echo "  branch figures are layout-sensitive and must be read with that"
    echo "  caveat stated; never present them as measurements."
    return 0
}
