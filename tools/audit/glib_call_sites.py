#!/usr/bin/env -S uv run --no-project --managed-python --with clang==21.1.7 python -X lazy_imports=normal
"""Regenerate the GLib/libc call-site TSV from a pinned libpinyin checkout.

The checkout must be at 074a2219c90feaf962d0d24f034514033ece5f99. Each DBM
backend is built twice: once with the pin's own flags, recorded by `bear`, for
clang to parse into call sites; once at -O0 with the linker removing what no
export or `main` reaches (-dead_strip on macOS, --gc-sections on Linux), so its
map says which functions each library and utility keeps. `family` is the one
hand-kept column, in glib_call_sites.families.tsv. The tools and the procedure
are in docs/runbooks/glib-call-sites.md.
"""
from __future__ import annotations

import argparse
import csv
import importlib.metadata
import io
import json
import os
import re
import shutil
import subprocess
import sys
import ctypes
from dataclasses import dataclass, field
from pathlib import Path

PIN = "074a2219c90feaf962d0d24f034514033ece5f99"
ROOT = Path(__file__).resolve().parents[2]
FAMILIES = ROOT / "tools/audit/glib_call_sites.families.tsv"
OUTPUT = ROOT / "docs/findings/glib-libc-call-sites.tsv"
HEADER = ("family", "library", "facade", "backend", "pin_site", "pin_call",
          "call")
FAMILIES_HEADER = ("function", "family")
CELLS = {"bdb": "BerkeleyDB", "kc": "KyotoCabinet", "tkrzw": "Tkrzw"}
LLVM_MAJOR = "21"
# Homebrew keg names; `llvm@21` is the keg whose LLVM matches the bindings the
# recipe pins, and libpinyin's configure asks for Berkeley DB 5.
FORMULAE = ("autoconf", "automake", "bear", "berkeley-db@5", "gettext", "glib",
            "kyoto-cabinet", "libtool", f"llvm@{LLVM_MAJOR}", "tkrzw")
LINUX_COMMANDS = ("autoreconf", "automake", "bear", "c++filt", "g++", "libtoolize",
                  "make", "pkg-config")
LINUX_PACKAGES = ("glib-2.0", "kyotocabinet", "tkrzw")
LINUX_DB_HEADER = "/usr/include/db.h"
BUCKETS = ("pinyin", "zhuyin", "tools")
SECTION = re.compile(r"^ (\.text\S*)(?:\s+(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+(\S.*))?$")
CONTINUATION = re.compile(r"^\s+(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+(\S.*)$")
SECTION_PREFIX = re.compile(r"^\.text\.(?:(?:unlikely|startup|exit|hot)\.)?")
MACHO_OBJECT = re.compile(r"^\[\s*(\d+)\]\s+(\S.*)$")
MACHO_SYMBOL = re.compile(r"^(?:0x[0-9A-Fa-f]+|<<dead>>)\s+(?:0x([0-9A-Fa-f]+)\s+)\[\s*(\d+)\]\s+(\S.*)$")


@dataclass(frozen=True)
class Toolchain:
    """What differs between the macOS and Linux runs."""
    libclang: str
    resource_dir: str
    parse_flags: tuple[str, ...]
    reach_cxxflags: str
    reach_ldflags: str
    environment: dict[str, str]
    configure_ldflags: str = ""
    symbol_prefix: str = ""
    real_cxxflags: str = ""


def brew_opt() -> Path:
    """Homebrew's `opt` directory, without running Homebrew."""
    return Path(os.environ.get("HOMEBREW_PREFIX", "/opt/homebrew")) / "opt"


def missing_tools() -> list[str]:
    """The tools this platform's run needs and does not have, by name."""
    if sys.platform == "darwin":
        opt = brew_opt()
        return [name for name in FORMULAE if not (opt / name).exists()]
    missing = [name for name in LINUX_COMMANDS if shutil.which(name) is None]
    library = f"libclang-{LLVM_MAJOR}.so.1"
    try:
        ctypes.CDLL(library)
    except OSError:
        missing.append(library)
    resource = f"/usr/lib/llvm-{LLVM_MAJOR}/lib/clang/{LLVM_MAJOR}"
    if not Path(resource).is_dir():
        missing.append(resource)
    if shutil.which("pkg-config"):
        missing += [name for name in LINUX_PACKAGES if subprocess.run(
            ["pkg-config", "--exists", name], check=False).returncode]
    if not Path(LINUX_DB_HEADER).exists():
        missing.append(LINUX_DB_HEADER)
    return missing


def toolchain() -> Toolchain:
    """Paths and flags for this platform; the tools are already checked."""
    if sys.platform == "darwin":
        opt = brew_opt()
        llvm = opt / f"llvm@{LLVM_MAJOR}"
        sdk = subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()
        kegs = ("glib", "gettext", "berkeley-db@5", "kyoto-cabinet", "tkrzw")
        includes = " ".join(f"-I{opt / keg}/include" for keg in kegs)
        # tkrzw links lz4, lzma and zstd from Homebrew's shared lib directory.
        libraries = " ".join([*(f"-L{opt / keg}/lib" for keg in kegs), f"-L{opt.parent}/lib"])
        path = f"{opt / 'libtool'}/libexec/gnubin:{os.environ['PATH']}"
        return Toolchain(
            str(llvm / "lib/libclang.dylib"),
            str(llvm / f"lib/clang/{LLVM_MAJOR}"),
            ("-isysroot", sdk, "-stdlib=libc++"),
            # Tkrzw's headers need C++17, GCC's default and not Apple clang's.
            "-O0 -std=gnu++17",
            f"-Wl,-dead_strip -Wl,-map,$@.map {libraries}",
            {"CPPFLAGS": includes, "PATH": path,
             "PKG_CONFIG_PATH": ":".join(f"{opt / keg}/lib/pkgconfig" for keg in kegs)},
            libraries,
            "_",
            "-g -O2 -std=gnu++17")
    return Toolchain(
        f"libclang-{LLVM_MAJOR}.so.1",
        f"/usr/lib/llvm-{LLVM_MAJOR}/lib/clang/{LLVM_MAJOR}",
        (),
        "-O0 -ffunction-sections",
        "-Wl,--gc-sections -Wl,-Map=$@.map",
        {})


def check_pin(pin: Path) -> None:
    """Require the checkout to report the pinned commit."""
    try:
        head = subprocess.check_output(
            ["git", "-C", str(pin), "rev-parse", "HEAD"], text=True,
            stderr=subprocess.PIPE).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise ValueError(f"{pin} is not a git checkout: {error}") from error
    if head != PIN:
        raise ValueError(f"{pin} is at {head}, expected {PIN}")


def read_families(path: Path) -> dict[str, str]:
    """Read the function-to-family table."""
    with path.open(newline="") as stream:
        reader = csv.DictReader(stream, delimiter="\t")
        if tuple(reader.fieldnames or ()) != FAMILIES_HEADER:
            raise ValueError(f"{path}: header must be {FAMILIES_HEADER}")
        table: dict[str, str] = {}
        for row in reader:
            if row["function"] in table:
                raise ValueError(f"{path}: duplicate function {row['function']}")
            table[row["function"]] = row["family"]
    return table


# Linker maps ---------------------------------------------------------------


def object_stem(path: str) -> str:
    """The object's stem, from `dir/.libs/x.o` or `dir/liby.a(x.o)`."""
    member = re.search(r"\(([^()]+)\)$", path)
    name = member.group(1) if member else path.rsplit("/", 1)[-1]
    return name.removesuffix(".o")


def parse_macho_map(text: str) -> tuple[set[tuple[str, str]], set[tuple[str, str]]]:
    """(kept, discarded) pairs of an Apple ld map, without Mach-O's leading `_`."""
    kept: set[tuple[str, str]] = set()
    discarded: set[tuple[str, str]] = set()
    objects: dict[str, str] = {}
    state = ""
    for line in text.splitlines():
        if line.startswith("# Object files:"):
            state = "objects"
        elif line.startswith("# Sections:"):
            state = ""
        elif line.startswith("# Symbols:"):
            state = "symbols"
        elif line.startswith("# Dead Stripped Symbols:"):
            state = "dead"
        elif line.startswith("#"):
            continue
        elif state == "objects":
            match = MACHO_OBJECT.match(line)
            if match:
                objects[match.group(1)] = object_stem(match.group(2).strip())
        elif state in ("symbols", "dead"):
            match = MACHO_SYMBOL.match(line)
            if match is None:
                continue
            size, number, symbol = match.groups()
            if int(size, 16) == 0 or number not in objects or not symbol.startswith("_"):
                continue
            pair = (objects[number], symbol[1:])
            (kept if state == "symbols" else discarded).add(pair)
    return kept, discarded


def parse_link_map(text: str) -> tuple[set[tuple[str, str]], set[tuple[str, str]]]:
    """(kept, discarded) (object stem, function symbol) pairs of a linker map."""
    if text.startswith("# Path:"):
        return parse_macho_map(text)
    kept: set[tuple[str, str]] = set()
    discarded: set[tuple[str, str]] = set()
    target = None
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.startswith("Discarded input sections"):
            target = discarded
            continue
        if line.startswith("Linker script and memory map"):
            target = kept
            continue
        match = SECTION.match(line)
        if target is None or match is None:
            continue
        name, _, size, path = match.groups()
        if path is None:
            following = lines[index + 1] if index + 1 < len(lines) else ""
            more = CONTINUATION.match(following)
            if more is None:
                continue
            _, size, path = more.groups()
        symbol = SECTION_PREFIX.sub("", name)
        if int(size, 16) == 0 or symbol == name:
            continue
        target.add((object_stem(path.strip()), symbol))
    return kept, discarded


def strip_template_arguments(text: str) -> str:
    """Remove balanced `<...>` groups, leaving operators spelled with < alone."""
    text = re.sub(r"operator\s*(<<=|>>=|<=>|<<|>>|<=|>=|<|>)",
                  lambda m: "operator@" + str(len(m.group(1))) + (
                      "l" if m.group(1)[0] == "<" else "g"), text)
    text = text.replace("operator->", "operator@a")
    out: list[str] = []
    depth = 0
    for character in text:
        if character == "<":
            depth += 1
        elif character == ">" and depth:
            depth -= 1
        elif depth == 0:
            out.append(character)
    return "".join(out)


def template_key(demangled: str) -> str | None:
    """`ns::Class::method/arity` of a demangled instantiation, without arguments."""
    if demangled.startswith(("vtable", "typeinfo", "VTT", "guard", "non-virtual",
                             "virtual thunk")):
        return None
    text = strip_template_arguments(demangled)
    end = text.rfind(")")
    if end < 0:
        return None
    depth = 0
    start = end
    while start >= 0:
        if text[start] == ")":
            depth += 1
        elif text[start] == "(":
            depth -= 1
            if depth == 0:
                break
        start -= 1
    if start < 0:
        return None
    params = text[start + 1:end].strip()
    arity = 0
    if params and params != "void":
        arity = 1
        depth = 0
        for character in params:
            if character == "(":
                depth += 1
            elif character == ")":
                depth -= 1
            elif character == "," and depth == 0:
                arity += 1
    name = text[:start].replace("(anonymous namespace)::", "").strip().split(" ")[-1]
    return f"{name}/{arity}"


@dataclass
class View:
    """What one link kept, by exact symbol and by template key."""
    pairs: set[tuple[str, str]] = field(default_factory=set)
    symbols: set[str] = field(default_factory=set)
    templates: set[str] = field(default_factory=set)

    def add(self, pairs: set[tuple[str, str]],
            demangled: dict[str, str]) -> None:
        self.pairs |= pairs
        for _, symbol in pairs:
            self.symbols.add(symbol)
            key = template_key(demangled.get(symbol, ""))
            if key:
                self.templates.add(key)


@dataclass(frozen=True)
class FunctionRef:
    """A function as clang names it: mangled, or a template pattern's key."""
    symbols: tuple[str, ...] = ()
    template: str = ""
    internal_to: str = ""

    def found_in(self, view: View) -> bool:
        if self.template:
            return self.template in view.templates
        if self.internal_to:
            return any((self.internal_to, s) in view.pairs for s in self.symbols)
        return any(s in view.symbols for s in self.symbols)


def symbol_candidates(mangled: str, kind: str) -> tuple[str, ...]:
    """The section names a constructor or destructor's mangled name can take."""
    if kind == "ctor":
        pattern, forms = r"C[1245]E", ("C1E", "C2E")
    elif kind == "dtor":
        pattern, forms = r"D[01245]E", ("D0E", "D1E", "D2E")
    else:
        return (mangled,)
    found = {mangled}
    for match in re.finditer(pattern, mangled):
        for form in forms:
            found.add(mangled[:match.start()] + form + mangled[match.end():])
    return tuple(sorted(found))


def facade_of(buckets: set[str]) -> str:
    """The facade value of the links that keep a function."""
    if {"pinyin", "zhuyin"} <= buckets:
        return "both"
    for name in ("pinyin", "zhuyin", "tools"):
        if name in buckets:
            return name
    return "none"


def backend_of(path: str) -> str:
    """The backend a source file belongs to, from its name alone."""
    stem = path.rsplit("/", 1)[-1]
    if "_bdb" in stem or stem.startswith("bdb_"):
        return "bdb"
    if "_kyotodb" in stem or stem.startswith("kyotodb_"):
        return "kc"
    if "_tkrzwdb" in stem or stem.startswith("tkrzwdb_"):
        return "tkrzw"
    return "all"


# Clang ----------------------------------------------------------------------


@dataclass(frozen=True)
class Site:
    """One GLib/libc name written at one source line."""
    file: str
    line: int
    name: str
    library: str
    text: str
    function: FunctionRef | None
    function_name: str = ""


def library_of(path: str) -> str:
    """Which library a declaration's header belongs to."""
    if "/glib-2.0/" in path:
        return "glib"
    if "/c++/" in path:
        return "c++"
    stem = path.rsplit("/", 1)[-1]
    if stem == "db.h" or stem.startswith("kc") or "/tkrzw" in path or stem.startswith("tkrzw"):
        return "dbm"
    if "/usr/include/" in path or "/gcc/" in path or "/clang/" in path:
        return "libc"
    return "unknown"


def call_text(source: bytes, offset: int) -> str:
    """The call from its name through its close parenthesis, on one line."""
    depth = 0
    quote = None
    escaped = False
    index = offset
    while index < len(source):
        character = chr(source[index])
        if quote:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == quote:
                quote = None
        elif character in "\"'":
            quote = character
        elif character == "(":
            depth += 1
        elif character == ")":
            depth -= 1
            if depth == 0:
                index += 1
                break
        elif depth == 0 and character == ";":
            break
        index += 1
    text = source[offset:index].decode("utf-8", "replace").replace("\\\n", " ")
    return " ".join(text.split()).removeprefix("::")


class Parser:
    """Reads one backend's translation units into sites."""

    def __init__(self, root: Path, families: dict[str, str], tools: Toolchain):
        import clang.cindex as cindex
        bindings = importlib.metadata.version("clang")
        if bindings.split(".")[0] != LLVM_MAJOR:
            raise ValueError(f"clang bindings {bindings} do not match LLVM {LLVM_MAJOR}")
        try:
            if not cindex.Config.loaded:
                cindex.Config.set_library_file(tools.libclang)
            self.lib = cindex.conf.lib
        except cindex.LibclangError as error:
            raise ValueError(f"cannot load {LIBCLANG}: {error}") from error
        self.ci = cindex
        self.tools = tools
        kinds = cindex.CursorKind
        self.kinds = kinds
        self.functions = {kinds.FUNCTION_DECL, kinds.CXX_METHOD, kinds.CONSTRUCTOR,
                          kinds.DESTRUCTOR, kinds.CONVERSION_FUNCTION,
                          kinds.FUNCTION_TEMPLATE}
        self.templates = {kinds.CLASS_TEMPLATE, kinds.FUNCTION_TEMPLATE,
                          kinds.CLASS_TEMPLATE_PARTIAL_SPECIALIZATION}
        self.root = root
        self.src = str(root / "src") + os.sep
        self.families = families
        self.sources: dict[str, bytes] = {}
        self.sites: list[Site] = []
        self.unknown: set[tuple[str, str, str, int]] = set()
        self.errors: list[str] = []

    def absolute(self, name: str) -> str:
        return os.path.normpath(os.path.join(self.cwd, name))

    def relative(self, name) -> str | None:
        if name is None:
            return None
        path = self.absolute(name if isinstance(name, str) else name.name)
        return path[len(str(self.root)) + 1:] if path.startswith(self.src) else None

    def source(self, relative: str) -> bytes:
        if relative not in self.sources:
            self.sources[relative] = (self.root / relative).read_bytes()
        return self.sources[relative]

    def spelling_location(self, cursor):
        ci = self.ci
        file, line, column, offset = (ci.c_object_p(), ci.c_uint(), ci.c_uint(),
                                      ci.c_uint())
        self.lib.clang_getSpellingLocation(cursor.location, ci.byref(file),
                                           ci.byref(line), ci.byref(column),
                                           ci.byref(offset))
        return (ci.File(file).name if file else None), line.value, offset.value

    def overloads(self, cursor) -> list:
        found = []
        for index in range(self.lib.clang_getNumOverloadedDecls(cursor)):
            declaration = self.lib.clang_getOverloadedDecl(cursor, index)
            declaration._tu = cursor._tu
            found.append(declaration)
        return found

    def callee(self, call) -> tuple[list, str]:
        """The declarations and name a call resolves to, dependent or not."""
        kinds = self.kinds
        referenced = call.referenced
        if referenced is not None and referenced.kind in self.functions:
            return [referenced], referenced.spelling
        if referenced is not None and referenced.kind == kinds.OVERLOADED_DECL_REF:
            found = self.overloads(referenced)
            return found, found[0].spelling if found else ""
        for current in call.get_children():
            for _ in range(6):
                if current.kind == kinds.OVERLOADED_DECL_REF:
                    found = self.overloads(current)
                    return found, found[0].spelling if found else ""
                if current.kind in (kinds.DECL_REF_EXPR, kinds.MEMBER_REF_EXPR):
                    inner = current.referenced
                    if inner is not None and inner.kind in self.functions:
                        return [inner], inner.spelling
                    if inner is not None and inner.kind == kinds.OVERLOADED_DECL_REF:
                        found = self.overloads(inner)
                        return found, found[0].spelling if found else ""
                    return [], ""
                children = list(current.get_children())
                if not children:
                    break
                current = children[0]
            break
        return [], ""

    def function_ref(self, cursor) -> FunctionRef:
        """Name a function definition the way the linker will see it."""
        kinds = self.kinds
        parts = []
        templated = cursor.kind in self.templates
        parent = cursor.semantic_parent
        while parent is not None and parent.kind != kinds.TRANSLATION_UNIT:
            templated = templated or parent.kind in self.templates
            if parent.spelling:
                parts.append(strip_template_arguments(parent.spelling))
            parent = parent.semantic_parent
        if templated:
            name = strip_template_arguments(cursor.spelling)
            arity = sum(child.kind == kinds.PARM_DECL for child in cursor.get_children())
            return FunctionRef(template="::".join([*reversed(parts), name]) + f"/{arity}")
        kind = {kinds.CONSTRUCTOR: "ctor", kinds.DESTRUCTOR: "dtor"}.get(cursor.kind, "")
        mangled = cursor.mangled_name.removeprefix(self.tools.symbol_prefix)
        symbols = symbol_candidates(mangled, kind)
        internal = cursor.linkage == self.ci.LinkageKind.INTERNAL
        file = self.relative(cursor.location.file) or ""
        owner = Path(file).stem if internal and file.endswith(".cpp") else ""
        return FunctionRef(symbols=symbols, internal_to=owner)

    def walk(self, cursor, function, label, defined) -> None:
        kinds = self.kinds
        if cursor.kind in self.functions and cursor.is_definition():
            function = self.function_ref(cursor), cursor.displayname
            defined.append((self.relative(cursor.location.file), cursor.extent.start.offset,
                            cursor.extent.end.offset, function))
        elif cursor.kind == kinds.CALL_EXPR and function is not None:
            self.record_call(cursor, function, label)
        for child in cursor.get_children():
            if child.location.file is None or self.relative(child.location.file):
                self.walk(child, function, label, defined)

    def record_call(self, cursor, function, label) -> None:
        declarations, name = self.callee(cursor)
        if not declarations or not name:
            return
        if any(self.relative(item.location.file) for item in declarations):
            return
        file = declarations[0].location.file
        if file is None:
            return
        library = library_of(self.absolute(file.name))
        if library == "dbm" or (library == "c++" and name not in self.families):
            return
        spelled, line, offset = self.spelling_location(cursor)
        relative = self.relative(spelled)
        if relative is None:
            return  # a macro's body in a system header: the macro is the site
        if library == "unknown" or name not in self.families:
            self.unknown.add((library, name, relative, line))
            return
        self.sites.append(Site(
            relative, line, name, "libc" if library == "c++" else library,
            call_text(self.source(relative), offset), function[0], function[1]))

    def macros(self, unit, defined) -> None:
        """Sites from table-named macros, including those inside project macros."""
        kinds = self.kinds
        system: dict[str, str] = {}
        project: dict[tuple[str, int], object] = {}
        expansions = []
        for top in unit.cursor.get_children():
            if top.location.file is None:
                continue
            relative = self.relative(top.location.file)
            if top.kind == kinds.MACRO_DEFINITION:
                if relative:
                    project[(relative, top.location.line)] = top
                else:
                    system[top.spelling] = self.absolute(top.location.file.name)
            elif top.kind == kinds.MACRO_INSTANTIATION and relative:
                expansions.append((relative, top))

        def enclosing(file: str, offset: int):
            best = None
            for owner, start, end, function in defined:
                if owner == file and start <= offset < end and (
                        best is None or end - start < best[0]):
                    best = (end - start, function)
            return best[1] if best else None

        users: dict[tuple[str, int], set] = {key: set() for key in project}
        for relative, top in expansions:
            definition = top.referenced
            if definition is None or definition.location.file is None:
                continue
            owner = self.relative(definition.location.file)
            function = enclosing(relative, top.extent.start.offset)
            if owner:
                users[(owner, definition.location.line)].add(function)
            elif top.spelling in self.families:
                self.sites.append(self.macro_site(
                    relative, top.location.line, top.spelling,
                    self.absolute(definition.location.file.name),
                    top.extent.start.offset, function))
        bodies = {}
        names: dict[str, list] = {}
        for key, definition in project.items():
            tokens = list(definition.get_tokens())[1:]
            bodies[key] = [(token.spelling, token.location.line, token.location.offset,
                            tokens[index + 1].spelling if index + 1 < len(tokens) else "")
                           for index, token in enumerate(tokens)]
            names.setdefault(definition.spelling, []).append(key)
        changed = True
        while changed:
            changed = False
            for key, tokens in bodies.items():
                for token, _, _, _ in tokens:
                    for other in names.get(token, ()):
                        if other != key and not users[key] <= users[other]:
                            users[other] |= users[key]
                            changed = True
        for key, tokens in bodies.items():
            for token, line, offset, following in tokens:
                if token in system and token in self.families and following == "(":
                    for function in users[key]:
                        self.sites.append(self.macro_site(
                            key[0], line, token, system[token], offset, function))

    def macro_site(self, file, line, name, header, offset, function) -> Site:
        library = "glib" if "/glib-2.0/" in header else "libc"
        ref, display = function if function else (None, "")
        return Site(file, line, name, library, call_text(self.source(file), offset),
                    ref, display)

    def parse(self, entry: dict) -> None:
        arguments = compile_arguments(entry, self.tools)
        self.cwd = str(Path(entry["directory"]).resolve())
        path = self.absolute(entry["file"])
        os.chdir(self.cwd)
        try:
            unit = self.ci.Index.create().parse(
                path, args=arguments,
                options=self.ci.TranslationUnit.PARSE_DETAILED_PROCESSING_RECORD)
        except self.ci.TranslationUnitLoadError as error:
            raise ValueError(f"{entry['file']} does not parse: {error}") from error
        self.errors += [f"{entry['file']}: {item}" for item in unit.diagnostics
                        if item.severity >= 3]
        defined: list = []
        kinds = self.kinds
        for top in unit.cursor.get_children():
            if top.kind in (kinds.MACRO_DEFINITION, kinds.MACRO_INSTANTIATION,
                            kinds.INCLUSION_DIRECTIVE):
                continue
            if top.location.file is None or self.relative(top.location.file):
                self.walk(top, None, entry["file"], defined)
        self.macros(unit, defined)


def compile_arguments(entry: dict, tools: Toolchain) -> list[str]:
    """The compile command's flags, for libclang."""
    arguments = entry["arguments"][1:]
    source = os.path.join(entry.get("directory", ""), entry["file"])
    result: list[str] = []
    index = 0
    while index < len(arguments):
        item = arguments[index]
        if item in ("-c", "-MD", "-MP"):
            index += 1
        elif item in ("-o", "-MT", "-MF"):
            index += 2
        elif os.path.normpath(os.path.join(entry.get("directory", ""), item)) == os.path.normpath(source):
            index += 1
        else:
            result.append(item)
            index += 1
    return ["-x", "c++", "-resource-dir", tools.resource_dir, *tools.parse_flags, *result]


# Build ----------------------------------------------------------------------


def run(command: list[str], cwd: Path, log: Path, tools: Toolchain) -> None:
    with log.open("a") as stream:
        result = subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT,
                                env={**os.environ, **tools.environment}, check=False)
    if result.returncode:
        raise ValueError(f"{' '.join(command)} failed in {cwd}; see {log}")


def build_cell(pin: Path, work: Path, dbm: str, jobs: int, tools: Toolchain) -> None:
    """Build one backend twice: the pin's flags under bear, then for the linker."""
    jobs_flag = f"-j{jobs}"
    for tree in ("real", "reach"):
        target = work / tree
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(pin, target, ignore=shutil.ignore_patterns(".git"))
        log = work / f"{tree}.log"
        log.unlink(missing_ok=True)
        run(["autoreconf", "--force", "--install"], target, log, tools)
        configure = ["./configure", "--disable-static", f"--with-dbm={dbm}",
                     "--enable-libzhuyin"]
        if tools.configure_ldflags:
            configure.append(f"LDFLAGS={tools.configure_ldflags}")
        if tree == "real":
            if tools.real_cxxflags:
                configure.append(f"CXXFLAGS={tools.real_cxxflags}")
            run(configure, target, log, tools)
            run(["bear", "--output", str(target / "compile_commands.json"), "--",
                 "make", "-C", "src", jobs_flag], target, log, tools)
        else:
            if tools.symbol_prefix:
                # The pin's libpinyin.exp omits five exports its libpinyin.ver has.
                # The version script is the ABI, so the links root the same set.
                for library in ("libpinyin", "libzhuyin"):
                    source = target / "src" / library
                    source.with_suffix(".exp").write_text(
                        apple_export_list(source.with_suffix(".ver").read_text()))
            run([*configure, f"CXXFLAGS={tools.reach_cxxflags}"], target, log, tools)
            ldflags = f"LDFLAGS={tools.reach_ldflags}"
            run(["make", "-C", "src", jobs_flag, ldflags], target, log, tools)
            run(["make", "-C", "utils", jobs_flag, ldflags], target, log, tools)


def apple_export_list(version_script: str) -> str:
    """An Apple ld export list with exactly the version script's global symbols."""
    block = version_script.split("global:", 1)[1].split("local:", 1)[0]
    return "".join(f"_{name}\n" for name in re.findall(r"([A-Za-z0-9_]+)\s*;", block))


def demangle(symbols: set[str]) -> dict[str, str]:
    """The demangled form of each symbol."""
    ordered = sorted(symbols)
    output = subprocess.run(["c++filt", "-n"], input="\n".join(ordered) + "\n", text=True,
                            capture_output=True, check=True).stdout.splitlines()
    return dict(zip(ordered, output, strict=True))


def read_views(reach: Path) -> tuple[dict[str, View], View]:
    """What each library and the utilities together kept, and what each link saw."""
    maps = {"pinyin": [reach / "src/libpinyin.la.map"],
            "zhuyin": [reach / "src/libzhuyin.la.map"],
            "tools": sorted((reach / "utils").rglob("*.map"))}
    parsed = {}
    for bucket, paths in maps.items():
        if not paths or not all(path.exists() for path in paths):
            raise ValueError(f"no linker map for {bucket} under {reach}")
        parsed[bucket] = [parse_link_map(path.read_text(errors="replace"))
                          for path in paths]
    symbols = {symbol for pairs in parsed.values() for kept, discarded in pairs
               for _, symbol in kept | discarded}
    names = demangle(symbols)
    views = {bucket: View() for bucket in BUCKETS}
    seen = View()
    for bucket, pairs in parsed.items():
        for kept, discarded in pairs:
            views[bucket].add(kept, names)
            seen.add(kept | discarded, names)
    return views, seen


# Merge and render -----------------------------------------------------------


@dataclass
class Row:
    """A row before rendering: every backend's occurrences of one site."""
    file: str
    line: int
    name: str
    library: str
    texts: set[str] = field(default_factory=set)
    buckets: set[str] = field(default_factory=set)
    placed: bool = False
    functions: set[str] = field(default_factory=set)


def merge_sites(rows: dict[tuple[str, int, str], Row], sites: list[Site],
                views: dict[str, View], seen: View) -> None:
    """Fold one backend's sites into the rows, placing each by its function."""
    for site in sites:
        key = (site.file, site.line, site.name)
        row = rows.setdefault(key, Row(site.file, site.line, site.name, site.library))
        row.texts.add(site.text)
        if site.function_name:
            row.functions.add(site.function_name)
        if site.function is None:
            continue
        row.placed = row.placed or site.function.found_in(seen)
        for bucket, view in views.items():
            if site.function.found_in(view):
                row.buckets.add(bucket)


def build_rows(rows: dict[tuple[str, int, str], Row],
               families: dict[str, str]) -> list[dict[str, str]]:
    """The TSV rows, sorted, with the table closed in both directions."""
    extra = sorted(set(families) - {row.name for row in rows.values()})
    if extra:
        raise ValueError(f"{len(extra)} table functions have no site: {', '.join(extra)}")
    result = []
    for row in rows.values():
        result.append({
            "family": families[row.name],
            "library": row.library,
            "facade": facade_of(row.buckets),
            "backend": backend_of(row.file),
            "pin_site": f"{row.file}:{row.line}",
            "pin_call": row.name,
            "call": sorted(row.texts)[0],
        })
    result.sort(key=lambda item: (
        item["family"], item["pin_site"].rsplit(":", 1)[0],
        int(item["pin_site"].rsplit(":", 1)[1]), item["pin_call"], item["call"]))
    return result


def render(rows: list[dict[str, str]]) -> str:
    """Render the TSV with the repository's CSV dialect."""
    buffer = io.StringIO()
    writer = csv.writer(buffer, delimiter="\t", quoting=csv.QUOTE_MINIMAL,
                        lineterminator="\n")
    writer.writerow(HEADER)
    for row in rows:
        writer.writerow([row[column] for column in HEADER])
    return buffer.getvalue()


def unplaced(rows: dict[tuple[str, int, str], Row]) -> list[str]:
    """Sites whose function no link ever saw, so the linker cannot place them."""
    lines = []
    for key in sorted(rows):
        row = rows[key]
        if not row.placed:
            names = ", ".join(sorted(row.functions)) or "(no enclosing function)"
            lines.append(f"{row.file}:{row.line}\t{row.name}\t{names}")
    return lines


def generate(work: Path, cells: list[str], families: dict[str, str],
             tools: Toolchain | None = None) -> tuple[dict[tuple[str, int, str], Row], list[str]]:
    """Read every backend's two built trees under `work` into rows."""
    tools = tools or toolchain()
    rows: dict[tuple[str, int, str], Row] = {}
    errors: list[str] = []
    unknown: set = set()
    start = Path.cwd()
    for cell in cells:
        real = work / cell / "real"
        database = json.loads((real / "compile_commands.json").read_text())
        parser = Parser(real.resolve(), families, tools)
        for entry in database:
            if Path(entry["directory"]).resolve() == (real / "src").resolve():
                parser.parse(entry)
        os.chdir(start)
        errors += parser.errors
        unknown |= parser.unknown
        views, seen = read_views(work / cell / "reach")
        merge_sites(rows, parser.sites, views, seen)
    if errors:
        raise ValueError(f"{len(errors)} parse errors, first: {errors[0]}")
    if unknown:
        listed = "; ".join(f"{name} ({library}) at {file}:{line}"
                           for library, name, file, line in sorted(unknown))
        raise ValueError(f"{len(unknown)} calls to functions outside the table: {listed}")
    return rows, unplaced(rows)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pin", type=Path, help="libpinyin checkout at the pin")
    parser.add_argument("--families", type=Path, default=FAMILIES)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--work", type=Path, default=ROOT / "target/glib-call-sites",
                        help="directory for the build trees")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--reuse", action="store_true",
                        help="read the trees already under --work")
    parser.add_argument("--unplaced", type=Path,
                        help="also write the unplaced sites to this file")
    parser.add_argument("--check", action="store_true",
                        help="compare with --output instead of writing it")
    args = parser.parse_args(argv)
    try:
        missing_names = missing_tools()
        if missing_names:
            raise ValueError("missing tools, install them first: "
                             + ", ".join(missing_names))
        check_pin(args.pin)
        tools = toolchain()
        families = read_families(args.families.resolve())
        output = args.output.resolve()
        work = args.work.resolve()
        pin = args.pin.resolve()
        if not args.reuse:
            for cell, dbm in CELLS.items():
                (work / cell).mkdir(parents=True, exist_ok=True)
                build_cell(pin, work / cell, dbm, args.jobs, tools)
        rows, missing = generate(work, list(CELLS), families, tools)
        text = render(build_rows(rows, families))
    except (OSError, ValueError, subprocess.CalledProcessError, ImportError) as error:
        print(f"glib_call_sites: {error}", file=sys.stderr)
        return 1
    print(f"glib_call_sites: {len(rows)} rows; the linker cannot place "
          f"{len(missing)} sites", file=sys.stderr)
    for line in missing:
        print(f"unplaced\t{line}", file=sys.stderr)
    if args.unplaced:
        args.unplaced.write_text("".join(line + "\n" for line in missing))
    if args.check:
        current = output.read_text() if output.exists() else ""
        if current != text:
            print(f"glib_call_sites: {output} is stale", file=sys.stderr)
            return 1
        return 0
    output.write_text(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
