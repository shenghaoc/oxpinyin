# Stable demangled identity -> exact function section. Invoked by
# function-order.sh; GNU nm's existing sysv format supplies both fields.
# No address, archive name, hash, or nm encounter order ranks a function.
function trim(s) {
    sub(/^[[:space:]]+/, "", s)
    sub(/[[:space:]]+$/, "", s)
    return s
}
function canonical(s) {
    # Legacy Rust hashes are not path identity. Preserve generics/closures.
    sub(/\.llvm\.[[:xdigit:]]+$/, "", s)
    if (match(s, /::h[[:xdigit:]]+$/) && RLENGTH == 19)
        s = substr(s, 1, RSTART - 1)
    return s
}
function rust_mangled(s) {
    # A raw Rust symbol is never a stable path: v0 (_R) carries crate
    # disambiguators and legacy (_ZN...17h<hash>E) a hash.
    sub(/\.llvm\.[[:xdigit:]]+$/, "", s)
    if (s ~ /^_R/)
        return 1
    return s ~ /^_ZN/ && match(s, /17h[0-9a-f]+E$/) && RLENGTH == 20
}
function cxx_mangled(s) {
    # An Itanium C++ name is its own stable path. The system C++ compiler
    # decides which C++ functions a build contains (inlining, GCC clones).
    return s ~ /^_Z/ && !rust_mangled(s)
}
function section_family(section, raw) {
    raw = section
    sub(/^\.text\.((unlikely|hot|startup|exit)\.)?/, "", raw)
    sub(/\.llvm\.[[:xdigit:]]+$/, "", raw)
    sub(/\.[0-9]+$/, "", raw)
    return raw
}
function stable_name(name, section, raw, tail, position, rest, length_text, crate, roots, root) {
    name = canonical(name)
    raw = section
    sub(/^\.text\.[a-z.]*_R/, "_R", raw)
    sub(/\.llvm\.[[:xdigit:]]+$/, "", raw)
    sub(/\.[0-9]+$/, "", raw)
    if (raw !~ /^_R/)
        return name
    # Rust v0's optional instantiating-crate root is omitted by GNU nm.
    # Match the decoded crate name, not its Cs<disambiguator> value.
    tail = raw
    while (match(tail, /Cs[0-9A-Za-z]+_[0-9]+[A-Za-z_][0-9A-Za-z_]*$/)) {
        position = RSTART
        rest = substr(tail, position)
        sub(/^Cs[0-9A-Za-z]+_/, "", rest)
        length_text = rest
        sub(/[^0-9].*$/, "", length_text)
        crate = substr(rest, length(length_text) + 1)
        if (length_text + 0 == length(crate)) {
            roots++
            root = crate
        }
        tail = substr(tail, position + 1)
    }
    if (roots == 1)
        name = name " [instantiating crate: " root "]"
    return name
}
function fail(message) {
    print "function ordering: " message > "/dev/stderr"
    bad = 1
}
mode != "inventory" && FILENAME == policy {
    n = split($0, fields, "\t")
    if (FNR == 1) {
        if (n != 2 || fields[1] != "oxpinyin-function-order-v1" || fields[2] != soname)
            fail("policy header does not match " soname)
        next
    }
    tier = fields[1]
    name = fields[2]
    if (n != 2 || (tier != "init" && tier != "steady") || name == "" ||
        name != trim(name) || name != canonical(name) || name ~ /[[:cntrl:]]/ ||
        rust_mangled(name) || (name in wanted)) {
        fail("invalid or duplicate policy row " FNR)
        next
    }
    if (tier == "init" && steady_seen)
        fail("init entries must precede steady entries")
    if (tier == "steady")
        steady_seen = 1
    if (cxx_mangled(name))
        cxx_rows++
    wanted[name] = ++count
    names[count] = name
    next
}
{
    # The last six separators have a fixed meaning; a demangled const
    # expression may itself contain a vertical bar.
    n = split($0, fields, "\\|")
    if (n < 7 || trim(fields[n - 3]) != "FUNC")
        next
    name = fields[1]
    for (i = 2; i <= n - 6; i++)
        name = name "|" fields[i]
    section = trim(fields[n])
    name = stable_name(trim(name), section)
    if (section !~ /^\.text\.[A-Za-z0-9_.$]+$/)
        next
    # Do not order a bundled section by just its first function.
    if (trim(fields[n - 5]) !~ /^0+$/) {
        bundled[section] = 1
        next
    }
    if (name == "" || name ~ /[[:cntrl:]]/ || rust_mangled(name))
        next
    identity = name SUBSEP section
    if (!(identity in seen)) {
        seen[identity] = 1
        entry_names[++entries] = name
        entry_sections[entries] = section
    }
}
END {
    for (i = 1; i <= entries; i++) {
        name = entry_names[i]
        section = entry_sections[i]
        if (section in bundled)
            continue
        if (mode == "inventory") {
            print name "\t" section
        } else if (name in wanted) {
            family = section_family(section)
            if ((name in resolved) && resolved[name] != family)
                ambiguous[name] = 1
            resolved[name] = family
            group_count[name]++
            group_sections[name SUBSEP group_count[name]] = section
        }
    }
    if (mode == "inventory")
        exit bad ? 1 : 0
    if (count == 0)
        fail("policy has no functions")
    if (mode == "policy") {
        if (bad)
            exit 1
        for (i = 1; i <= count; i++)
            print names[i]
        exit 0
    }
    for (i = 1; i <= count; i++) {
        name = names[i]
        if (!(name in resolved)) {
            # A stale Rust or C path fails (#1071). A C++ path this build's
            # compiler did not emit is skipped; function_order.py agrees.
            if (cxx_mangled(name)) {
                absent++
                continue
            }
            fail("unresolved policy path: " name)
        }
        if (name in ambiguous)
            fail("ambiguous policy path: " name)
    }
    if (bad)
        exit 1
    # Every resolve reports the count, 0 of 0 included, so a relink log
    # always shows whether the build's compiler dropped any C++ path.
    print "function ordering: " (absent + 0) " of " (cxx_rows + 0) \
        " C++ policy paths are not in this build; skipped" > "/dev/stderr"
    # This file contains output-section descriptions, WITHOUT a SECTIONS
    # wrapper: --section-ordering-file augments the default script.
    print ".text : {"
    for (i = 1; i <= count; i++) {
        name = names[i]
        # Include every recognized clone of the exact function family, in
        # input order. Never silently pick one body or sort by its hash/id.
        for (j = 1; j <= group_count[name]; j++) {
            section = group_sections[name SUBSEP j]
            if (!(section in emitted)) {
                print "  *(\"" section "\")"
                emitted[section] = 1
            }
        }
    }
    print "}"
}
