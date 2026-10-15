"""Mechanical tier classification for the pinned J coverage inventory.

Rust citation extraction is lexical, and clang supplies the actual pin body
extents. Module/file-level citations and test-only functions are excluded.
The random sample is selected from classified row identities, not names.
"""

import math
from pathlib import Path
import random
import re
import tomllib

RAW_STRING = re.compile(r'(?:br|r)(#*)"')
CHAR_LITERAL = re.compile(r"'(?:\\(?:u\{[^}]*\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'")


def lexical_mask(source):
    """Hide strings/comments while retaining offsets and newlines."""
    chars = list(source)
    comments = []
    i = 0
    while i < len(source):
        start = i
        if source.startswith("//", i):
            end = source.find("\n", i)
            i = len(source) if end < 0 else end
            comments.append((start, i, source[start:i]))
        elif source.startswith("/*", i):
            i += 2
            depth = 1
            while i < len(source) and depth:
                if source.startswith("/*", i):
                    depth += 1
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            comments.append((start, i, source[start:i]))
        else:
            raw = RAW_STRING.match(source, i) if source[i] in {"r", "b"} else None
            char = CHAR_LITERAL.match(source, i) if source[i] == "'" else None
            if raw:
                terminator = '"' + raw.group(1)
                end = source.find(terminator, raw.end())
                i = len(source) if end < 0 else end + len(terminator)
            elif source[i] == '"':
                i += 1
                while i < len(source):
                    if source[i] == "\\":
                        i += 2
                    elif source[i] == '"':
                        i += 1
                        break
                    else:
                        i += 1
            elif char:
                i = char.end()
            else:
                i += 1
                continue
        for j in range(start, min(i, len(chars))):
            if chars[j] != "\n":
                chars[j] = " "
    grouped = []
    for a, b, text in comments:
        if grouped and not source[grouped[-1][1]:a].strip():
            old_a, old_b, old_text = grouped[-1]
            grouped[-1] = (old_a, b, old_text + source[old_b:a] + text)
        else:
            grouped.append((a, b, text))
    return "".join(chars), grouped


def brace_end(mask, start):
    depth = 0
    for i in range(start, len(mask)):
        if mask[i] == "{":
            depth += 1
        elif mask[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
    raise ValueError("unclosed Rust body")


def rust_items(path, root):
    source = path.read_text()
    mask, comments = lexical_mask(source)
    test_spans = []
    for m in re.finditer(r"#\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*(?:pub\s+)?mod\s+\w+\s*\{", mask):
        brace = mask.find("{", m.start())
        test_spans.append((m.start(), brace_end(mask, brace)))
    for m in re.finditer(r"\bfn\s+(\w+)\s*(?:<[^;{}]*>)?\s*\(", mask):
        if any(a <= m.start() < b for a, b in test_spans):
            continue
        # Item attributes/modifiers and contiguous documentation immediately
        # before its fn, never a module-level citation separated by code.
        prefix_start = max(mask.rfind("}", 0, m.start()), mask.rfind(";", 0, m.start()), mask.rfind("{", 0, m.start())) + 1
        prefix = mask[prefix_start:m.start()]
        if re.search(r"#\[\s*(?:test|ignore|cfg\s*\(\s*test)", prefix):
            continue
        brace = mask.find("{", m.end())
        semicolon = mask.find(";", m.end())
        if brace < 0 or 0 <= semicolon < brace:
            continue  # trait declaration, not an implementation
        end = brace_end(mask, brace)
        associated = [(a, b, text) for a, b, text in comments if prefix_start <= a < m.start() or brace < a < end]
        for a, b, text in associated:
            # Prelude documentation must be contiguous with this item.
            if a < m.start():
                between = mask[b:m.start()]
                between = re.sub(r"#\[[^]]*\]|\b(?:pub|unsafe|async|const|extern)\b|\([^)]*\)", "", between)
                if between.strip():
                    continue
            yield {"file": str(path.relative_to(root)), "name": m.group(1),
                   "line": source.count("\n", 0, m.start()) + 1,
                   "end": source.count("\n", 0, end) + 1,
                   "comment_line": source.count("\n", 0, a) + 1,
                   "comment": text}


CITATION = re.compile(r"(?P<file>[\w./-]+\.(?:cpp|h)):(?P<start>\d+)(?:\s*[-–]\s*(?P<end>\d+))?")

# Reviewed at immutable 074a2219, phrase_index.cpp:150-178, against the
# shipping crates/oxpinyin-user/src/store.rs add_unigram_frequency and
# library_total_overflows helpers. Their comments describe a USER_FILE update
# combining facade-total accounting with the sub-index overflow guard; they
# do not supply the pin body's out-of-range/no-item/corruption branches.
# Keep that useful scoped citation, but do not promote it to a complete T1
# function counterpart. Execution evidence is classified independently.
CITATION_LIMITS = {
    ('c:@N@pinyin@S@SubPhraseIndex@F@add_unigram_frequency#i#i#',
     'crates/oxpinyin-user/src/store.rs', 'add_unigram_frequency'),
}


def pin_function_name(node):
    name = node["name"]
    if node.get("usr"):
        return name
    name = name.split("(", 1)[0]
    while re.search(r"<[^<>]*>", name):
        name = re.sub(r"<[^<>]*>", "", name)
    return name.split("::")[-1].split()[-1]


def shipping_crates(root):
    """Local normal-dependency closure of the two shipping facade roots."""
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"].get("dependencies", {})
    pending = [root / "crates/oxpinyin-capi", root / "crates/oxpinyin-zhuyin-capi"]
    result = set()
    while pending:
        directory = pending.pop().resolve()
        if directory in result:
            continue
        result.add(directory)
        manifest = tomllib.loads((directory / "Cargo.toml").read_text())
        for name, spec in manifest.get("dependencies", {}).items():
            if not isinstance(spec, dict):
                continue
            if spec.get("workspace"):
                spec = workspace[name]
                base = root
            else:
                base = directory
            if isinstance(spec, dict) and "path" in spec:
                pending.append(base / spec["path"])
    return result


def cited_items(root, pin, nodes, tightened=False):
    files = {str(p.relative_to(pin)) for p in (pin / "src").rglob("*") if p.suffix in {".cpp", ".h"}}
    by_suffix = {}
    for file in files:
        for suffix in (file, file.removeprefix("src/"), Path(file).name):
            by_suffix.setdefault(suffix, set()).add(file)
    bodies = {}
    for key, node in nodes.items():
        if node.get("body_start") is not None:
            bodies.setdefault(node["file"], []).append((key, node))
    matches = {}
    shipping = shipping_crates(root)
    for path in sorted((root / "crates").rglob("*.rs")):
        if any(part in {"tests", "examples", "benches"} for part in path.relative_to(root).parts):
            continue
        if tightened == "role" and (root / "crates" / path.relative_to(root / "crates").parts[0]).resolve() not in shipping:
            continue
        if path.name == "build.rs" or path.stem.endswith(("_tests", "_test")):
            continue
        for item in rust_items(path, root):
            for cite in CITATION.finditer(item["comment"]):
                candidates = by_suffix.get(cite.group("file"), set())
                if len(candidates) != 1:
                    continue
                file = next(iter(candidates))
                lo = int(cite.group("start"))
                hi = int(cite.group("end") or lo)
                for key, node in bodies.get(file, []):
                    if not node["body_start"] <= lo <= hi <= node["body_end"]:
                        continue
                    if tightened:
                        # A call-site citation inside an unrelated pin caller
                        # is not enough: require the comment to name this
                        # function as well as citing its actual body.
                        name = pin_function_name(node)
                        if not re.search(r"(?<![A-Za-z0-9_])" + re.escape(name) + r"(?![A-Za-z0-9_])", item["comment"]):
                            continue
                        if tightened == "role":
                            same_name = item["name"].strip("_").removeprefix("get_") == name.strip("_").removeprefix("get_")
                            claim = re.search(r"(?is)\b(?:port of|mirrors?|equivalent of|counterpart of|implements)\s+[^.\n]{0,80}" + re.escape(name), item["comment"])
                            if not same_name and not claim:
                                continue
                            if (key, item['file'], item['name']) in CITATION_LIMITS:
                                continue
                    record = dict(item, pin_file=file, pin_start=lo, pin_end=hi, citation=cite.group(0))
                    matches.setdefault(key, []).append(record)
    return {key: sorted(items, key=lambda x: (x["file"], x["line"], x["comment_line"], x["citation"]))[0] for key, items in matches.items()}


def sample_rows(rows, seed):
    keys = sorted(rows)
    return random.Random(seed).sample(keys, math.ceil(len(keys) * 0.05))


# Facility families are intentionally limited to storage/lifetime primitives.
# Their semantics and Rust implementation anchors are named once per family.
FACILITIES = {
    "chunk-storage": {
        "targets": [("crates/oxpinyin-data/src/phrase_library.rs", "pub(crate) const fn as_slice("),
                    ("crates/oxpinyin-data/src/phrase_library.rs", "fn drop(&mut self)"),
                    ("crates/oxpinyin-data/src/chunk_write.rs", "pub fn build_chunk("),
                    ("crates/oxpinyin-data/src/phrase_library.rs", "fn map_chunk("),
                    ("crates/oxpinyin-user/src/persistence.rs", "fn write_chunk_file("),
                    ("crates/oxpinyin-data/src/chunk_format.rs", "pub fn build_memory_chunk(")],
        "reason": "MemoryChunk's raw ownership and byte-buffer operations are replaced by owned Vec-backed typed items, borrowed payload slices, and MappedFile's munmap Drop. The runtime build_chunk writer serializes typed fields, and build_memory_chunk frames payload length/checksum. No generic mutable MemoryChunk API exists; reads are bounded slice accesses and mutations operate on typed fields before serialization. This family accounts for storage/lifetime mechanisms, not the checksum algorithm or every failure diagnostic.",
    },
    "unaligned-words": {
        "targets": [("crates/oxpinyin-data/src/phrase_library.rs", "fn u32_at("),
                    ("crates/oxpinyin-data/src/row_format.rs", "pub const fn encode_token_key(")],
        "reason": "UnalignedMemory<unsigned int> uses memcpy to load/store a word. Rust explicitly assembles u32 from slice bytes and emits to_le_bytes; no pointer alignment is required. The data format rejects unsupported big-endian targets, so the pin's host-endian word and Rust's little-endian word agree on the supported bdb cell.",
    },
    "length-entry-ownership": {
        "targets": [("crates/oxpinyin-data/src/chewing_table.rs", "pub(crate) fn new(dbm:"),
                    ("crates/oxpinyin-data/src/row_format.rs", "pub fn decode_items(")],
        "reason": "ChewingTableEntry<L>'s empty constructor/implicit MemoryChunk destructor and the array of sixteen reusable entries are absent. Rust owns only the DBM and decoded Vec<PinyinIndexItem> rows; row length is runtime metadata. Vec ownership supplies construction/destruction. Search and index mutation bodies are not covered by this lifetime family.",
    },
    "prefix-comparator-state": {
        "targets": [("crates/oxpinyin-data/src/chewing_table.rs", "pub fn prefix_keys_match(")],
        "reason": "PrefixLessThanWithTones<L>'s constructor stores a scalar prefix length, and its destructor only resets that scalar. Rust obtains the same length from the query slice and has no owning comparator object; the destructor's zero is never observed after destruction. The comparison algorithm itself is reviewed separately.",
    },
}


def facility(node):
    file, line = node["file"], node["line"]
    if file == "src/include/memory_chunk.h" and line != 136:
        return "chunk-storage"
    if file == "src/include/unaligned_memory.h":
        return "unaligned-words"
    if file == "src/storage/chewing_large_table2.h" and line in {74, 86}:
        return "length-entry-ownership"
    if file == "src/storage/chewing_large_table2.cpp" and line in {26, 68}:
        return "length-entry-ownership"
    if file == "src/storage/chewing_large_table2.h" and line in {52, 55}:
        return "prefix-comparator-state"
    return None
