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
function fail(message) {
    print "function ordering: " message > "/dev/stderr"
    bad = 1
}
FILENAME == policy {
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
        name ~ /^(_R|_ZN[0-9])/ || (name in wanted)) {
        fail("invalid or duplicate policy row " FNR)
        next
    }
    if (tier == "init" && steady_seen)
        fail("init entries must precede steady entries")
    if (tier == "steady")
        steady_seen = 1
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
    name = canonical(trim(name))
    section = trim(fields[n])
    if (section !~ /^\.text\.[A-Za-z0-9_.$]+$/)
        next
    # Do not order a bundled section by just its first function.
    if (trim(fields[n - 5]) !~ /^0+$/) {
        bundled[section] = 1
        next
    }
    if (name == "" || name ~ /[[:cntrl:]]/ || name ~ /^(_R|_ZN[0-9])/)
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
            if ((name in resolved) && resolved[name] != section)
                ambiguous[name] = 1
            resolved[name] = section
        }
    }
    if (mode == "inventory")
        exit bad ? 1 : 0
    if (count == 0)
        fail("policy has no functions")
    for (i = 1; i <= count; i++) {
        name = names[i]
        if (!(name in resolved))
            fail("unresolved policy path: " name)
        if (name in ambiguous)
            fail("ambiguous policy path: " name)
    }
    if (bad)
        exit 1
    # This file contains output-section descriptions, WITHOUT a SECTIONS
    # wrapper: --section-ordering-file augments the default script.
    print ".text : {"
    for (i = 1; i <= count; i++) {
        section = resolved[names[i]]
        if (!(section in emitted)) {
            print "  *(\"" section "\")"
            emitted[section] = 1
        }
    }
    print "}"
}
