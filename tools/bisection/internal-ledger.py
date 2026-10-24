#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Generate the axis-J source coverage ledgers without saving captures.

The bdb cell is the full ledger. The kc and tkrzw cells are delta ledgers
(--cell kc|tkrzw --delta-from <bdb ledger>): only the functions defined in
the cell's DBM-conditional sources and the shared functions whose export
reachability differs from bdb are measured; every other row is inherited
from the bdb ledger by reference.

Only compiler bitcode intermediates and the requested Markdown ledger are written.
LLVM text, opt output and libclang identities stay in memory. The independent
export roots come from nm, not from a checked-in expected count.
"""

import argparse
from collections import defaultdict, deque
import hashlib
import json
from pathlib import Path
import platform
import re
import shlex
import subprocess
import sys

from clang_identity import Clang
from coverage_tiers import cited_items, facility, sample_rows, FACILITIES
import gcov_measure

PIN = "074a2219c90feaf962d0d24f034514033ece5f99"
REPO = Path(__file__).resolve().parents[2]

# Explicit source-reviewed roles, selected by exact USR (not Rust name matches).
# Each anchor must occur exactly once; its current line is regenerated. These
# are structural counterparts, not a claim of complete ABI input parity.
MAPPINGS = {
    "c:@N@pinyin@S@MemoryChunk@F@get_check_sum#*1C#i#": (
        "crates/oxpinyin-data/src/chunk_format.rs", "pub fn chunk_checksum(",
        "Both XOR little-endian u32 words, then fold tail bytes at successive 8-bit shifts; the checksum covers the same payload bytes.",
    ),
    "c:@S@_ChewingKey@F@get_table_index#": (
        "crates/oxpinyin-chewing/src/chewing_key.rs", "pub fn table_index(",
        "Both index the ported table by (initial * number-of-middles + middle) * number-of-finals + final and map the -1 sentinel to zero. Invalid-dimension abort behavior needs separate policy/probe review.",
    ),
    "c:@S@_ChewingKey@F@is_valid_zhuyin#": (
        "crates/oxpinyin-core/src/scheme.rs", "fn is_valid_zhuyin(",
        "The pin indexes valid_zhuyin_table by components and tone; Rust tests the ported per-key valid-tone mask. Invalid-component abort behavior needs separate policy/probe review.",
    ),
    "c:@S@_ChewingKey@F@get_pinyin_string#": (
        "crates/oxpinyin-chewing/src/chewing_key.rs", "pub fn pinyin_string(",
        "Both render the content-table pinyin column and append a digit exactly for a nonzero valid tone; ownership/allocation is handled by the calling C facade.",
    ),
    "c:@S@_ChewingKey@F@get_shengmu_string#": (
        "crates/oxpinyin-chewing/src/chewing_key.rs", "pub fn shengmu_string(",
        "Both return the content-table shengmu column without a tone suffix; the Rust borrowed string is copied by the calling C facade.",
    ),
    "c:@S@_ChewingKey@F@get_yunmu_string#": (
        "crates/oxpinyin-chewing/src/chewing_key.rs", "pub fn yunmu_string(",
        "Both return the content-table yunmu column without a tone suffix; the Rust borrowed string is copied by the calling C facade.",
    ),
    "c:@S@_ChewingKey@F@get_zhuyin_string#": (
        "crates/oxpinyin-chewing/src/chewing_key.rs", "pub fn zhuyin_string(",
        "Both render the content-table zhuyin column, omit zero/first tone, and append the same tone-mark table entry for tones 2 through 5. Invalid-tone abort behavior needs separate policy/probe review.",
    ),
    "c:@S@_ChewingKey@F@get_luoma_pinyin_string#": (
        "crates/oxpinyin-chewing/src/chewing_key.rs", "pub fn luoma_pinyin_string(",
        "Both render the content-table Luoma column and append the nonzero valid tone as a digit, including first tone; allocation is in the calling C facade.",
    ),
    "c:@S@_ChewingKey@F@get_secondary_zhuyin_string#": (
        "crates/oxpinyin-chewing/src/chewing_key.rs", "pub fn secondary_zhuyin_string(",
        "Both render the content-table secondary-zhuyin column and append the nonzero valid tone as a digit, including first tone; allocation is in the calling C facade.",
    ),
}


def counterpart(key):
    """Render an explicit Rust role mapping, or a GAP when none is registered.

    Read the mapped file and require exactly one anchor match, otherwise
    raise RuntimeError.
    """
    if key not in MAPPINGS:
        return "GAP — semantic counterpart/absence has not been source-reviewed"
    file, anchor, reason = MAPPINGS[key]
    lines = (REPO / file).read_text().splitlines()
    matches = [i + 1 for i, line in enumerate(lines) if anchor in line]
    if len(matches) != 1:
        raise RuntimeError(f"counterpart anchor not unique: {file}: {anchor}")
    line = matches[0]
    return f"ROLE MAPPED — [{file}:{line}](../../{file}#L{line}); {reason}"


def run(args, **kwargs):
    """Capture command text and return stdout, raising RuntimeError on nonzero exit."""
    p = subprocess.run([str(x) for x in args], capture_output=True, text=True, **kwargs)
    if p.returncode:
        raise RuntimeError(f"{args[0]} failed ({p.returncode}): {p.stderr.strip()}")
    return p.stdout


def globals_in(path):
    """Read pinyin/zhuyin export names from semicolon-terminated version-script entries."""
    return set(re.findall(r"^\s*((?:pinyin|zhuyin)_\w+)\s*;", path.read_text(), re.M))


def exported(prefix, library):
    """Read defined facade-prefixed dynamic symbols with nm, stripping symbol versions."""
    text = run(["nm", "-D", "--defined-only", prefix / "lib" / f"lib{library}.so.15"])
    return {s.split("@", 1)[0] for s in re.findall(r"\b[TwW]\s+(\S+)", text) if s.startswith(library + "_")}


def sources(pin, cell="bdb"):
    """Read the autoconf source list, selecting only one cell's DBM conditional."""
    return gcov_measure.cell_sources(pin, cell)


def symbol_base(symbol):
    # C1/C2 and D0/D1/D2 are ABI entry variants of one source function.
    """Normalize constructor/destructor ABI variants to one source-function spelling."""
    return re.sub(r"([CD])[012](?=E)", r"\1X", symbol)


def normalized_type(text):
    """Remove whitespace and canonicalize a leading const before pointer/reference types."""
    text = re.sub(r"\s+", "", text)
    return re.sub(r"const([A-Za-z_][A-Za-z_0-9:]*(?:<[^<>]*>)?)([&*])", r"\1const\2", text)


def callback_prototypes(signature):
    """Extract normalized bool/int/void function-pointer prototypes from a signature."""
    return {normalized_type(m.group(1) + "(" + m.group(2) + ")") for m in re.finditer(r"\b(bool|int|void)\s*\(\*\)\s*\(([^()]*)\)", signature)}


def internal_callback_pool(nodes, edges):
    """Group explicit function-address bindings by complete normalized C++ type.

    Return the candidate pool and unresolved identity set. Ignore nonstatic
    member pointers; incomplete function-kind or template/type information
    keeps a binding unresolved.
    """
    pool, unknown = defaultdict(set), set()
    for caller, target, kind in edges:
        if kind != "LLVM-address/callback" or target not in nodes or not nodes[target].get("pin", True):
            continue
        node = nodes[target]
        if node.get("function_kind") is None:
            unknown.add(target)
            continue
        if node.get("function_kind") == 21 and "is_static" not in node:
            unknown.add(target)
            continue
        if node.get("function_kind") not in {8, 30} and not node.get("is_static"):
            continue  # member pointers are not ordinary comparator pointers
        typ = node.get("canonical_type")
        unresolved_template = typ and any(not re.fullmatch(r"[0-9uUlL,\s]+", arg) for arg in re.findall(r"<([^<>]*)>", typ))
        if typ and "type-parameter" not in typ and not unresolved_template:
            pool[normalized_type(typ)].add(target)
        else:
            unknown.add(target)
    return pool, unknown


def singleton_callback(node, pool, unknown, roots_have_no_callbacks):
    """Resolve one private typed callback only when all binding guards hold.

    Require an external-header caller, one pinyin prototype and candidate, no
    unknown bindings, and no callback inputs at export roots; otherwise return None.
    """
    prototypes = callback_prototypes(node["signature"])
    if not roots_have_no_callbacks or unknown or not node["file"].startswith("external-header/") or len(prototypes) != 1:
        return None
    prototype = next(iter(prototypes))
    candidates = pool.get(prototype, set())
    if "pinyin::" not in prototype or len(candidates) != 1:
        return None
    return next(iter(candidates)), prototype


def llvm(pin, bc, ast):
    """Combine LLVM symbols, source metadata, and exact libclang identities.

    Run llvm-dis, llvm-cxxfilt, and opt without saving their text. Return
    definitions, symbol-to-key bindings, direct/address edges, external calls,
    and indirect call sites. Normalize ABI variants and resolve aliases;
    retain full signatures and translation units when USR matching is unavailable.
    """
    ir = run(["llvm-dis", bc, "-o", "-"])
    meta = {m.group(1): m.group(2) for m in re.finditer(r"^!(\d+) = (.*)$", ir, re.M)}
    defs, by_symbol, bodies = {}, {}, {}
    ast_symbols, referenced_symbols = defaultdict(list), defaultdict(list)
    for n in ast.values():
        if n["symbol"]:
            referenced_symbols[symbol_base(n["symbol"])].append(n)
        if n.get("definition") and n["symbol"]:
            ast_symbols[symbol_base(n["symbol"])].append(n)
    functions = list(re.finditer(r"^define .*?@(?:\"([^\"]+)\"|([^ (]+))\(.*?^}", ir, re.M | re.S))
    symbols = [m.group(1) or m.group(2) for m in functions]
    demangled = run(["llvm-cxxfilt"], input="\n".join(symbols) + "\n").splitlines()
    for m, symbol, signature in zip(functions, symbols, demangled, strict=True):
        body = m.group(0)
        debug = re.search(r"!dbg !(\d+)", body.splitlines()[0])
        sub = meta[debug.group(1)] if debug else ""
        file_ref = re.search(r"file: !(\d+)", sub)
        line = re.search(r"\bline: (\d+)", sub)
        is_pin = False
        if file_ref and line:
            file_meta = meta[file_ref.group(1)]
            filename = re.search(r'filename: "([^"]+)"', file_meta).group(1)
            directory = re.search(r'directory: "([^"]*)"', file_meta).group(1)
            path = Path(filename) if filename.startswith("/") else Path(directory) / filename
            try:
                file = str(path.resolve().relative_to(pin))
                is_pin = True
            except ValueError:
                # Traverse emitted standard-library/GLib inline bodies too:
                # they can call back into pin comparators. No host path is
                # printed; the header basename is a display label, never an
                # identity or edge-resolution key.
                file = "external-header/" + Path(filename).name
            line = int(line.group(1))
        else:
            file, line = "compiler-generated", 0
        candidates = [n for n in ast_symbols[symbol_base(symbol)] if n["file"] == file and n["line"] == line]
        if is_pin and len(candidates) == 1:
            node = dict(candidates[0])
            key = node["usr"]
        else:
            # Instantiations not exposed by libclang retain their actual full
            # compiler-demangled signature and source definition identity.
            key = f"signature:{file}:{line}:{signature}"
            if not is_pin:
                key = f"external-symbol:{symbol}"
            node = {"usr": "", "signature": signature, "file": file, "line": line, "name": signature, "definition": True}
            refs = [n for n in referenced_symbols[symbol_base(symbol)] if n["file"] == file and n["line"] == line]
            if len(refs) == 1:
                # The referenced specialization's exact mangling identifies
                # its compiler template origin without changing baseline IDs.
                node["template_usr"] = refs[0].get("template_usr", "")
                node["instantiation_usr"] = refs[0]["usr"]
                node["is_static"] = refs[0].get("is_static", False)
                if not node["template_usr"] and refs[0].get("template_owner_usr"):
                    # libclang identifies the specialized owning class. The
                    # immutable declaration point (including column) selects
                    # its actual primary member; overload names are not used.
                    origins = [n for n in ast.values() if n.get("definition") and n["owner_usr"] == refs[0]["template_owner_usr"] and (n["file"], n["line"], n["col"]) == (file, refs[0]["line"], refs[0]["col"])]
                    if len(origins) == 1:
                        node["template_usr"] = origins[0]["usr"]
        node["key"] = key
        node["pin"] = is_pin
        unit_ref = re.search(r"unit: !(\d+)", sub)
        unit = meta.get(unit_ref.group(1), "") if unit_ref else ""
        unit_file_ref = re.search(r"file: !(\d+)", unit)
        unit_file = meta.get(unit_file_ref.group(1), "") if unit_file_ref else ""
        unit_name = re.search(r'filename: "([^"]+)"', unit_file)
        tu = Path(unit_name.group(1)).name if unit_name else ""
        binding = (symbol_base(re.sub(r"\.\d+$", "", symbol)), tu)
        node["llvm_bindings"] = defs.get(key, {}).get("llvm_bindings", set()) | {binding}
        defs[key] = node
        by_symbol[symbol] = key
        bodies[symbol] = body
    aliases = {}
    for m in re.finditer(r'^@(?:"([^"]+)"|([^\s=]+))\s*=\s*[^\n]*\balias\b[^\n]*\bptr\s+@(?:"([^"]+)"|([^\s,]+))', ir, re.M):
        aliases[m.group(1) or m.group(2)] = m.group(3) or m.group(4)
    for alias in aliases:
        target, visited = alias, set()
        while target in aliases and target not in visited:
            visited.add(target)
            target = aliases[target]
        if target in by_symbol:
            by_symbol[alias] = by_symbol[target]
    cg = subprocess.run(["opt", "-passes=print-callgraph", "-disable-output", str(bc)], capture_output=True, text=True)
    if cg.returncode:
        raise RuntimeError(cg.stderr)
    edges, external, indirect = set(), set(), []
    current = None
    for line in cg.stderr.splitlines():
        m = re.match(r"Call graph node for function: '([^']+)'", line)
        if line.startswith("Call graph node"):
            current = m.group(1) if m else None
        m = re.search(r"calls function '([^']+)'", line)
        if m and current in by_symbol:
            target = m.group(1)
            if target in by_symbol:
                edges.add((by_symbol[current], by_symbol[target], "LLVM-direct"))
            else:
                external.add((by_symbol[current], target))
    for symbol, body in bodies.items():
        caller = by_symbol[symbol]
        for line in body.splitlines():
            is_call = bool(re.search(r"\b(call|invoke)\b", line))
            if not is_call and not re.search(r"\bstore\s+ptr\s+@", line):
                continue
            # Function-address arguments are possible callbacks, explicitly
            # included rather than dropped at the external library boundary.
            references = re.findall(r"@(?:\"([^\"]+)\"|([\w.$]+))", line)
            direct = re.search(r'\b(?:call|invoke)\b.*?@(?:"([^"]+)"|([\w.$]+))\(', line) if is_call else None
            if direct:
                target = direct.group(1) or direct.group(2)
                if target in by_symbol:
                    edges.add((caller, by_symbol[target], "LLVM-direct"))
                else:
                    external.add((caller, target))
            if is_call and re.search(r"\b(?:call|invoke)\b.*?@(?:\"[^\"]+\"|[\w.$]+)\(", line):
                references = references[1:]  # callee is a direct edge, not a callback
            for target in references:
                target = target[0] or target[1]
                if target in by_symbol and target != symbol:
                    edges.add((caller, by_symbol[target], "LLVM-address/callback"))
            if is_call and re.search(r"\b(?:call|invoke)\b[^@\n]*%[\w.]+\(", line):
                dbg = re.search(r"!dbg !(\d+)", line)
                loc = meta.get(dbg.group(1), "") if dbg else ""
                ln = re.search(r"line: (\d+)", loc)
                indirect.append((caller, int(ln.group(1)) if ln else 0))
    return defs, by_symbol, edges, external, indirect


def witness(nodes, edges, roots):
    """Find deterministic breadth-first witness paths from sorted export roots.

    Return node -> (export, path of node/edge-kind pairs), ignoring edges with
    endpoints absent from nodes.
    """
    graph = defaultdict(set)
    for a, b, kind in edges:
        if a in nodes and b in nodes:
            graph[a].add((b, kind))
    paths, queue = {}, deque()
    for export, key in sorted(roots.items()):
        paths[key] = (export, [(key, "root")])
        queue.append(key)
    while queue:
        a = queue.popleft()
        for b, kind in sorted(graph[a]):
            if b not in paths:
                export, path = paths[a]
                paths[b] = (export, path + [(b, kind)])
                queue.append(b)
    return paths


def format_path(nodes, path):
    """Render a witness with each node's full signature, source site, and edge kind."""
    return " → ".join(f"{nodes[k]['signature']} @ {nodes[k]['file']}:{nodes[k]['line']} ({kind})" for k, kind in path)


def check_rows(expected, actual):
    """Return whether identity key sets agree, plus missing and extra key sets."""
    missing, extra = set(expected) - set(actual), set(actual) - set(expected)
    return not missing and not extra, missing, extra


def mutation_check(rows, nodes):
    """Exact preregistered deletions, in memory; never mutate source or disk."""
    pristine = "\n".join(rows[k] for k in sorted(rows)).encode()
    checks = []
    for name, predicate in [
        ("J-m1", lambda n: n["file"] == "src/pinyin.cpp" and n["name"] == "pinyin_init"),
        ("J-m2", lambda n: n["file"] == "src/pinyin.cpp" and n["line"] == 172 and n["name"] == "check_format"),
    ]:
        keys = [k for k, n in nodes.items() if k in rows and predicate(n)]
        if len(keys) != 1:
            raise RuntimeError(f"{name}: exact preregistered row not unique: {keys}")
        mutant = dict(rows)
        removed = mutant.pop(keys[0])
        passed, missing, extra = check_rows(rows, mutant)
        if passed or missing != {keys[0]} or extra:
            raise RuntimeError(f"{name}: deletion escaped detection")
        mutant[keys[0]] = removed
        restored = "\n".join(mutant[k] for k in sorted(mutant)).encode()
        if not check_rows(rows, mutant)[0] or restored != pristine:
            raise RuntimeError(f"{name}: restoration changed ledger bytes")
        checks.append(f"{name}: exact deleted row rejected; restored row set accepted, byte-identical")
    return checks


def escape(s):
    """Make a value fit one Markdown table cell by escaping pipes and flattening newlines."""
    return str(s).replace("|", "&#124;").replace("\n", " ")


def pin_link(node):
    """Link a pin node to its immutable source site; leave external locations as text."""
    loc = f"{node['file']}:{node['line']}"
    if not node.get("pin", True):
        return loc
    return f"[{loc}](https://github.com/libpinyin/libpinyin/blob/{PIN}/{node['file']}#L{node['line']})"


def main(contract_results=None, gate_receipt=None):
    """Compile a fresh pin call graph and dispatch ledger generation or validation.

    Validate pin, cell, exports, and oracle provenance; write lane-owned LLVM
    intermediates. Static preview returns graph locals; inventory/tier modes
    avoid behavioral campaigns. Other modes measure or reconcile and write
    or compare the requested ledger. Optional in-memory results require a
    gate receipt, read from stdin if omitted; invalid prerequisites fail.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pin", type=Path, required=True)
    parser.add_argument("--cell", choices=sorted(gcov_measure.CELLS), default="bdb", help="the pin's DBM cell: its Makefile.am conditional and config.h")
    parser.add_argument("--delta-from", type=Path, help="bdb ledger to inherit from; emits only the cell's conditional-source rows and the shared rows whose reachability differs (kc/tkrzw only)")
    parser.add_argument("--config", type=Path, required=True, help="existing autoconf config.h of the selected cell (read-only)")
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--build", type=Path, required=True, help="lane-owned bitcode directory")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true", help="compare generated ledger to output, without writing")
    parser.add_argument("--compare-ref", help="annotate each changed table row against this full commit SHA, in the single authorized ledger")
    parser.add_argument("--require-complete", action="store_true", help="fail on any semantic gap or unresolved graph target")
    parser.add_argument("--static-preview", action="store_true", help="return graph state for compiler-backed tooling tests without measurement")
    parser.add_argument("--inventory-check", action="store_true", help="independently rebuild compiler row identities and rerun exact deletion/restoration checks without repeating behavioral suites")
    parser.add_argument("--refresh-unknowns", action="store_true", help="retain published MATCH rows, independently rebuild identities and replay registered aborts plus J target controls")
    parser.add_argument("--tier-preview", action="store_true", help="print citation-tier candidates and random samples without writing the ledger")
    parser.add_argument("--sample-seed", type=int, default=10063648693516068216, help="reported seed for the citation-tier 5 percent sample")
    parser.add_argument("--measurement-build", type=Path)
    parser.add_argument("--measurement-config", type=Path)
    parser.add_argument("--pinyin-so", type=Path)
    parser.add_argument("--zhuyin-so", type=Path)
    parser.add_argument("--surface-pinyin-so", type=Path, help="separate debug-profile C ABI for expensive surface/corpus replays")
    parser.add_argument("--rust-target", type=Path)
    parser.add_argument("--export", type=Path)
    parser.add_argument("--model", type=Path)
    args = parser.parse_args()
    if (args.cell != "bdb") != (args.delta_from is not None):
        raise RuntimeError("--delta-from <bdb ledger> is required for the kc and tkrzw cells and is not accepted for bdb")
    if args.cell != "bdb" and (args.tier_preview or args.inventory_check or args.refresh_unknowns or args.compare_ref):
        raise RuntimeError("--tier-preview, --inventory-check, --refresh-unknowns and --compare-ref belong to the bdb ledger only")
    if contract_results is not None and gate_receipt is None:
        # Private API continuation: the orchestrator transfers its completed
        # unchanged gate observation through stdin, never a capture file.
        gate_receipt = json.loads(input('PRESERVED_SENTENCE_GATE_RECEIPT\n'))
    args.pin, args.config, args.oracle, args.build = (p.resolve() for p in (args.pin, args.config, args.oracle, args.build))
    if run(["git", "-C", args.pin, "rev-parse", "HEAD"]).strip() != PIN:
        raise RuntimeError("pin commit mismatch")
    if run(["git", "-C", args.pin, "status", "--porcelain", "--untracked-files=no"]).strip():
        raise RuntimeError("tracked pin source is modified")
    provenance = None
    if args.cell != "bdb":
        from delta_ledger import prefix_provenance
        provenance = prefix_provenance(args)
    gcov_measure.verify_prefix(args.oracle, provenance, args.cell)
    config = args.config.read_text()
    macro = gcov_measure.CELLS[args.cell]["macro"]
    others = "|".join(sorted(c["macro"] for c in gcov_measure.CELLS.values() if c["macro"] != macro))
    if not re.search(r"^#define " + macro + r"\b", config, re.M) or re.search(r"^#define (?:" + others + r")\b", config, re.M):
        raise RuntimeError(f"config is not the autoconf {args.cell} cell")
    if not args.config.name == "config.h":
        raise RuntimeError("--config must name config.h")
    flags = ["-std=c++17", "-DHAVE_CONFIG_H", f"-I{args.config.parent}"] + [f"-I{args.pin / 'src' / s}" for s in ("", "include", "storage", "lookup")]
    flags += shlex.split(run(["pkg-config", "--cflags", "glib-2.0"]))
    # The clang driver supplies GCC's libstdc++ paths; libclang needs the same
    # resolved include list explicitly (system paths vary across platforms).
    include_probe = subprocess.run(["clang++", "-E", "-x", "c++", "-", "-v"], input="", capture_output=True, text=True)
    includes = include_probe.stderr.split("#include <...> search starts here:", 1)[1].split("End of search list.", 1)[0]
    astflags = flags + ["-isystem" + s.strip() for s in includes.splitlines() if s.strip()]
    args.build.mkdir(parents=True, exist_ok=True)
    clang = Clang()
    ast, astedges, concrete, overrides, fields = {}, set(), set(), set(), []
    bases, abstract_classes, receiver_sites = set(), set(), defaultdict(set)
    tus = sources(args.pin, args.cell) + ["pinyin.cpp", "zhuyin.cpp"]
    objects = {}
    for tu in tus:
        source = args.pin / "src" / tu
        print(f"parse/compile {tu}", file=sys.stderr, flush=True)
        n, e, v, o, f, metadata = clang.extract(source, astflags, args.pin)
        for k, node in n.items():
            if node.get("definition") or k not in ast:
                ast[k] = node
        astedges.update(e)
        concrete.update(v)
        overrides.update(o)
        fields.extend(f)
        bases.update(metadata["bases"])
        abstract_classes.update(metadata["abstract_classes"])
        for caller, line, target, receiver in metadata["virtual_sites"]:
            if receiver:
                receiver_sites[caller, line, target].add(receiver)
        bc = args.build / (tu.replace("/", "-") + ".bc")
        command = ["clang++", *flags, "-O0", "-g", "-fno-inline", "-fno-discard-value-names", "-emit-llvm", "-c", source, "-o", bc]
        # Recompilation is intentional: no stale-input manifest or capture
        # file is used to decide whether an intermediate remains valid.
        run(command)
        objects[tu] = bc
    nodes, edges, external, indirect, roots = {}, set(), set(), [], {}
    expected_exports = set()
    for library in ("pinyin", "zhuyin"):
        names = exported(args.oracle, library)
        expected = globals_in(args.pin / "src" / f"lib{library}.ver")
        subject = globals_in(REPO / "crates" / f"oxpinyin-{'zhuyin-' if library == 'zhuyin' else ''}capi" / f"lib{library}.ver")
        if names != expected or names != subject:
            raise RuntimeError(f"{library}: oracle/pin/subject version-script root sets differ")
        expected_exports.update(names)
        linked = args.build / f"{library}.bc"
        run(["llvm-link", *[objects[x] for x in sources(args.pin, args.cell)], objects[library + ".cpp"], "-o", linked])
        n, symbols, e, ext, ind = llvm(args.pin, linked, ast)
        for key, node in n.items():
            node["llvm_bindings"] = node.get("llvm_bindings", set()) | nodes.get(key, {}).get("llvm_bindings", set())
            nodes[key] = node
        edges.update(e)
        external.update(ext)
        indirect.extend(ind)
        for name in names:
            if name not in symbols:
                raise RuntimeError(f"missing compiler export root: {name}")
            roots[name] = symbols[name]
    # AST-only bodies close inline edges LLVM did not emit as calls. Generic
    # template bodies remain distinct from specialized LLVM signatures.
    for k, n in ast.items():
        if n.get("definition") and k not in nodes:
            nodes[k] = dict(n, key=k, pin=True)
    derived = defaultdict(set)
    parent_classes = defaultdict(set)
    for child, parent in bases:
        parent_classes[child].add(parent)
    for base, child in overrides:
        derived[base].add(child)

    def subclass(child, parent):
        """Test reflexive or transitive inheritance in the extracted class graph."""
        pending, seen = [child], set()
        while pending:
            c = pending.pop()
            if c == parent:
                return True
            if c in seen:
                continue
            seen.add(c)
            pending.extend(parent_classes[c])
        return False

    def targets(usr):
        """Collect concrete virtual methods reachable through the override chain."""
        result, pending = set(), [usr]
        visited = set()
        while pending:
            u = pending.pop()
            if u in visited:
                continue
            visited.add(u)
            if u in concrete:
                result.add(u)
            pending.extend(derived[u])
        return result

    ambiguities, resolved_virtual, virtual_sites = [], set(), defaultdict(set)
    for a, b, kind, line in astedges:
        if kind != "virtual":
            edges.add((a, b, kind))
            continue
        choices = targets(b)
        if ast.get(b, {}).get("name", "").startswith("~"):
            choices = {choice for choice in choices if ast.get(choice, {}).get("owner_usr") not in abstract_classes}
        receivers = receiver_sites[a, line, b]
        if len(receivers) == 1:
            receiver = next(iter(receivers))
            choices = {choice for choice in choices if subclass(ast.get(choice, {}).get("owner_usr", ""), receiver)}
        virtual_sites[a, line].update(choices)
        if len(choices) == 1:
            resolved_virtual.add((a, next(iter(choices))))
            edges.add((a, next(iter(choices)), "AST-single-virtual"))
        elif choices:
            ambiguities.append((a, b, tuple(sorted(choices))))
            for choice in choices:
                edges.add((a, choice, "AST-conservative-virtual"))
        else:
            ambiguities.append((a, b, ()))
    paths = witness(nodes, edges, roots)
    reachable = {k: nodes[k] for k in paths if nodes[k].get("pin", True)}
    site_bodies, site_kinds, site_types = {}, defaultdict(set), defaultdict(set)
    for node in ast.values():
        site = (node["file"], node["line"])
        if node.get("definition"):
            site_kinds[site].add(node.get("function_kind"))
            if node.get("canonical_type"):
                site_types[site].add(node["canonical_type"])
        if node.get("body_start") is not None:
            site_bodies.setdefault(site, set()).add((node["body_start"], node["body_end"]))
    for node in nodes.values():
        site = (node["file"], node["line"])
        ranges = site_bodies.get(site, set())
        if len(ranges) == 1:
            node["body_start"], node["body_end"] = next(iter(ranges))
        if len(site_kinds[site]) == 1:
            node.setdefault("function_kind", next(iter(site_kinds[site])))
        if not node.get("canonical_type") and node.get("function_kind") in {8, 30}:
            # Non-template functions can have link-time local-symbol suffixes;
            # a template's compiler-demangled signature supplies its concrete
            # return/argument types rather than the generic source parameter.
            signature = re.sub(r"\s+\(\.\d+\)$", "", node["signature"])
            concrete_type = re.match(r"^(bool|int|void)\s+[^()]+\(([^()]*)\)$", signature)
            if concrete_type:
                node["canonical_type"] = concrete_type.group(1) + "(" + concrete_type.group(2) + ")"
            elif len(site_types[site]) == 1:
                node["canonical_type"] = next(iter(site_types[site]))
    if args.tier_preview:
        for tightened in ("role",):
            cited = cited_items(REPO, args.pin, reachable, tightened=tightened)
            print(f"citation-tier tightened={tightened} rows={len(cited)} sample_seed={args.sample_seed}")
            for key in sample_rows(cited, args.sample_seed):
                node, item = reachable[key], cited[key]
                print(f"SAMPLE {key}\nPIN {node['file']}:{node['body_start']}-{node['body_end']} {node['signature']}\nRUST {item['file']}:{item['line']}-{item['end']} {item['name']} COMMENT {item['comment_line']} {item['citation']}")
        print("facility candidates", sum(facility(n) is not None for n in reachable.values()))
        return
    direct_paths = witness(nodes, {e for e in edges if e[2] in {"LLVM-direct", "AST-direct"}}, roots)
    pinned_paths = witness(nodes, {e for e in edges if e[2] in {"LLVM-direct", "AST-direct", "AST-single-virtual"}}, roots)
    direct_count = sum(k in direct_paths for k in reachable)
    pinned_count = sum(k in pinned_paths and k not in direct_paths for k in reachable)
    conservative_count = len(reachable) - direct_count - pinned_count
    # A single concrete method in the exact override chain pins the virtual
    # call target. Multiple/absent targets and uncorrelated LLVM sites stay open.
    sites, resolved_sites, singleton_callbacks = [], [], []
    callback_pool, unknown_bindings = internal_callback_pool(nodes, edges)
    root_no_callback_inputs = all(key in ast and ast[key].get("canonical_type") and "(*)" not in normalized_type(ast[key]["canonical_type"]) for key in roots.values())
    field_sites = defaultdict(set)
    for a, line, parent, field, field_usr, field_type, header in fields:
        field_sites[a, line].add(f"{parent}::{field}")
    for k, line in sorted(set((k, line) for k, line in indirect if k in paths)):
        choices = virtual_sites[k, line]
        if len(choices) == 1:
            resolved_sites.append((k, line, next(iter(choices))))
        else:
            # The internal record comparator signature is carried by the
            # actual std template specialization, and its sole binding is a
            # private pin function address. No C export accepts a callback.
            # Multi-binding and erased/untyped prototypes remain unresolved.
            binding = singleton_callback(nodes[k], callback_pool, unknown_bindings, root_no_callback_inputs)
            if binding:
                singleton_callbacks.append((k, line, *binding))
            else:
                sites.append((k, line))
    ambiguous = sorted(set((a, b, choices) for a, b, choices in ambiguities if a in paths))
    if args.static_preview:
        return locals()
    if args.cell != "bdb":
        from delta_ledger import render_delta
        return render_delta(args, locals(), provenance)
    if args.refresh_unknowns:
        from finish_ledger import refresh
        return refresh(args, locals())
    if args.inventory_check:
        from render_ledger import baseline_rows
        import html
        original = args.output.read_text()
        baseline = baseline_rows(original)
        expected = dict(reachable)
        expected.update({key: nodes[key] for key in baseline if key not in expected and key in nodes})
        actual = {}
        for line in original.splitlines():
            if line.startswith('| function/'):
                cells = [html.unescape(cell.strip()) for cell in line.strip('|').split('|')]
                actual[cells[3]] = line
        ok, missing, extra = check_rows(expected, actual)
        if not ok:
            raise RuntimeError(f'compiler inventory differs: missing={sorted(missing)} extra={sorted(extra)}')
        from compiler_cells import validate
        from finish_ledger import table_rows
        validate(sys.modules[__name__], locals(), expected, table_rows(original))
        before = args.output.read_bytes()
        for result in mutation_check(actual, expected): print(result)
        if args.output.read_bytes() != before:
            raise RuntimeError('mutation/restoration changed the tracked ledger')
        print(f'pristine independent inventory check PASS: roots={len(roots)} rows={len(expected)}; tracked bytes unchanged')
        return
    from render_ledger import render
    return render(args, locals(), contract_results, gate_receipt)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError) as e:
        print(str(e), file=sys.stderr)
        sys.exit(1)
