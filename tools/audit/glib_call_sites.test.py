#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the GLib/libc call-site generator."""
import ctypes
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

SPEC = importlib.util.spec_from_file_location(
    "glib_call_sites", Path(__file__).with_name("glib_call_sites.py"))
driver = importlib.util.module_from_spec(SPEC)
sys.modules["glib_call_sites"] = driver
SPEC.loader.exec_module(driver)

FIXTURE = Path(__file__).with_name("fixtures") / "small"


class LinkMapTests(unittest.TestCase):
    def test_constructor_and_destructor_names_cover_each_section_form(self):
        self.assertIn("_ZN1AC2Ev", driver.symbol_candidates("_ZN1AC1Ev", "ctor"))
        self.assertIn("_ZN1AD0Ev", driver.symbol_candidates("_ZN1AD1Ev", "dtor"))
        self.assertEqual(driver.symbol_candidates("_Z1fv", ""), ("_Z1fv",))

    def test_a_template_key_ignores_arguments_and_return_type(self):
        cases = {
            "pinyin::Trellis<2, 3>::prepare(int)": "pinyin::Trellis::prepare/1",
            "bool pinyin::top<2>(unsigned long, _GPtrArray*)": "pinyin::top/2",
            "Holder<2>::used()": "Holder::used/0",
            "Holder<2>::Holder(void)": "Holder::Holder/0",
            "A<int>::operator<(A<int> const&) const": "A::operator@1l/1",
            "typeinfo for A<int>": None,
        }
        for demangled, key in cases.items():
            self.assertEqual(driver.template_key(demangled), key, demangled)


class RowTests(unittest.TestCase):
    def test_the_backend_comes_from_the_file_name(self):
        cases = {"src/storage/ngram_bdb.cpp": "bdb", "src/storage/bdb_utils.h": "bdb",
                 "src/storage/punct_table_kyotodb.h": "kc",
                 "src/storage/tkrzwdb_utils.h": "tkrzw", "src/pinyin.cpp": "all"}
        for path, backend in cases.items():
            self.assertEqual(driver.backend_of(path), backend)

    def merged(self):
        view = driver.View()
        view.add({("pinyin", "_Z1fv")}, {})
        empty = driver.View()
        seen = driver.View()
        seen.add({("pinyin", "_Z1fv"), ("pinyin", "_Z1gv")}, {})
        kept = driver.FunctionRef(("_Z1fv",))
        dropped = driver.FunctionRef(("_Z1gv",))
        unseen = driver.FunctionRef(("_Z1hv",))
        rows = {}
        driver.merge_sites(rows, [
            driver.Site("src/b.cpp", 2, "g_free", "glib", "g_free(x)", dropped, "g()"),
            driver.Site("src/a.cpp", 9, "g_free", "glib", 'g_free("a\tb")', kept, "f()"),
            driver.Site("src/a.cpp", 10, "memset", "libc", "memset(a, 0, 1)", unseen, "h()"),
        ], {"pinyin": view, "zhuyin": empty, "tools": empty}, seen)
        return rows

    def test_a_table_function_without_a_site_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "1 table functions have no site: strlen"):
            driver.build_rows(self.merged(),
                              {"g_free": "m", "memset": "b", "strlen": "b"})

    def test_a_duplicate_table_row_is_an_error(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "t.tsv"
            path.write_text("function\tfamily\nf\ta\nf\tb\n")
            with self.assertRaisesRegex(ValueError, "duplicate function f"):
                driver.read_families(path)

    def test_the_pin_check_rejects_another_commit(self):
        with mock.patch("subprocess.check_output", return_value=driver.PIN + "\n"):
            driver.check_pin(Path("."))
        with mock.patch("subprocess.check_output", return_value="0" * 40 + "\n"):
            with self.assertRaises(ValueError):
                driver.check_pin(Path("."))

    def test_compile_arguments_keep_only_what_the_parse_needs(self):
        entry = {"file": "a.cpp", "arguments": [
            "/usr/bin/g++", "-DX", "-I.", "-MT", "a.lo", "-MD", "-MP", "-MF", "a.d",
            "-c", "a.cpp", "-O2", "-o", "a.o"]}
        tools = driver.Toolchain("lib", "/resource", ("-isysroot", "/sdk"), "", "", {})
        self.assertEqual(driver.compile_arguments(entry, tools), [
            "-x", "c++", "-resource-dir", "/resource", "-isysroot", "/sdk",
            "-DX", "-I.", "-O2"])

    def test_compile_arguments_drop_an_absolute_file_entry(self):
        entry = {"directory": "/w/src", "file": "/w/src/a.cpp",
                 "arguments": ["g++", "-DX", "-c", "a.cpp"]}
        tools = driver.Toolchain("lib", "/r", (), "", "", {})
        self.assertEqual(driver.compile_arguments(entry, tools)[4:], ["-DX"])

    def test_the_apple_export_list_comes_from_the_version_script(self):
        script = "LIBPINYIN {\n  global:\n    a_one;\n    b_two;\n  local:\n    *;\n};\n"
        self.assertEqual(driver.apple_export_list(script), "_a_one\n_b_two\n")

    def test_every_missing_homebrew_tool_is_named(self):
        with tempfile.TemporaryDirectory() as directory:
            for name in driver.FORMULAE:
                if name not in ("bear", "tkrzw"):
                    (Path(directory) / "opt" / name).mkdir(parents=True)
            with mock.patch.object(driver.sys, "platform", "darwin"), \
                    mock.patch.dict(os.environ, {"HOMEBREW_PREFIX": directory}):
                self.assertEqual(driver.missing_tools(), ["bear", "tkrzw"])


class RecipeTests(unittest.TestCase):
    def test_just_recipe_preserves_a_pin_path_with_spaces(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pin = root / 'pin checkout with spaces'
            pin.mkdir()
            uv = root / 'uv'
            uv.write_text(
                f'#!{sys.executable}\n'
                'import json, sys\n'
                'if sys.argv[1:3] == ["python", "install"]: sys.exit(0)\n'
                f'if sys.argv[-1] == "--print-clang-requirement": print({driver.CLANG_REQUIREMENT!r}); sys.exit(0)\n'
                'print(json.dumps(sys.argv[1:]))\n')
            uv.chmod(0o755)
            result = subprocess.run(
                ['just', '--justfile', str(ROOT / 'justfile'), 'audit-glib-call-sites', str(pin)],
                env=dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH']),
                capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)[-10:], [
                'run', '--no-project', '--managed-python', '--with', driver.CLANG_REQUIREMENT,
                'python', '-X', 'lazy_imports=normal', 'tools/audit/glib_call_sites.py', str(pin)])


EXPECTED = """\
family\tlibrary\tfacade\tbackend\tpin_site\tpin_call\tcall
bytes\tlibc\tzhuyin\tall\tsrc/zhuyin.cpp:4\tstrlen\t"strlen(""same name"")"
bytes\tlibc\tpinyin\tall\tsrc/pinyin.cpp:4\tstrcmp\t"strcmp(""same name"", ""other library"")"
bytes\tlibc\tpinyin\tall\tsrc/shared.h:12\tmemset\tmemset(m_bytes, 0, N)
bytes\tlibc\tnone\tall\tsrc/shared.h:13\tmemcpy\tmemcpy(m_bytes, m_bytes, N)
io\tlibc\tnone\tall\tsrc/common.cpp:13\tfprintf\t"fprintf(stderr, ""dead\\n"")"
io\tlibc\tpinyin\tall\tsrc/common.cpp:5\tprintf\t"printf(""pinyin\\n"")"
memory\tglib\ttools\tall\tsrc/common.cpp:9\tg_strdup\t"g_strdup(""tool"")"
memory\tglib\tpinyin\tall\tsrc/shared.h:20\tg_free\tg_free(pointer)
memory\tglib\tboth\tall\tsrc/shared.h:7\tg_free\t"g_free(g_strdup(""both""))"
memory\tglib\tboth\tall\tsrc/shared.h:7\tg_strdup\t"g_strdup(""both"")"
memory\tglib\tnone\tall\tsrc/shared.h:17\tg_strdup\t"g_strdup(""nobody"")"
text\tglib\tpinyin\tall\tsrc/pinyin.cpp:14\tg_utf8_next_char\tg_utf8_next_char(pointer)
""".splitlines()
UNPLACED = [
    "src/shared.h:13\tmemcpy\tuninstantiated()",
    "src/shared.h:17\tg_strdup\tnever_called()",
]


def end_to_end_missing():
    """The tools this end-to-end test needs and does not have, by name."""
    missing = [name for name in ("uv", "c++", "c++filt", "pkg-config", "ld")
               if shutil.which(name) is None]
    if sys.platform == "darwin":
        opt = driver.brew_opt()
        missing += [name for name in ("glib", f"llvm@{driver.LLVM_MAJOR}")
                    if not (opt / name).exists()]
        if shutil.which("xcrun") is None:
            missing.append("xcrun")
        environment = ({**os.environ, **driver.toolchain().environment}
                       if "xcrun" not in missing else dict(os.environ))
    else:
        environment = dict(os.environ)
        library = f"libclang-{driver.LLVM_MAJOR}.so.1"
        try:
            ctypes.CDLL(library)
        except OSError:
            missing.append(library)
        resource = f"/usr/lib/llvm-{driver.LLVM_MAJOR}/lib/clang/{driver.LLVM_MAJOR}"
        if not Path(resource).is_dir():
            missing.append(resource)
    if "pkg-config" not in missing and subprocess.run(
            ["pkg-config", "--exists", "glib-2.0"], env=environment).returncode:
        missing.append("glib-2.0")
    return missing


if sys.platform in ("linux", "darwin"):
    class EndToEndTests(unittest.TestCase):
        """Clang and the platform linker on a small tree: the fixture stands for the pin."""

        def setUp(self):
            missing = end_to_end_missing()
            if missing:
                self.fail("missing tools: " + ", ".join(missing))

        def build(self, work):
            real = work / "bdb/real"
            reach = work / "bdb/reach"
            tools = driver.toolchain()
            env = {**os.environ, **tools.environment}
            for tree in (real, reach):
                shutil.copytree(FIXTURE / "src", tree / "src")
            shutil.copytree(FIXTURE / "utils", reach / "utils")
            glib = subprocess.check_output(["pkg-config", "--cflags", "glib-2.0"], text=True,
                                           env=env).split()
            libs = subprocess.check_output(["pkg-config", "--libs", "glib-2.0"], text=True,
                                           env=env).split()
            sources = ("common", "pinyin", "zhuyin")
            database = [{"directory": str((real / "src").resolve()), "file": f"{name}.cpp",
                         "arguments": ["g++", "-I.", *glib, "-O2", "-c", f"{name}.cpp"]}
                        for name in sources]
            (real / "compile_commands.json").write_text(json.dumps(database))
            source = reach / "src"
            cxxflags = tools.reach_cxxflags.split()
            for name in sources:
                subprocess.run(["c++", *cxxflags, "-fPIC", "-w", *glib, "-c", f"{name}.cpp"],
                               cwd=source, check=True, env=env)
            for library in ("pinyin", "zhuyin"):
                if sys.platform == "darwin":
                    link = ["-dynamiclib", "-Wl,-dead_strip", f"-Wl,-map,lib{library}.la.map",
                            f"-Wl,-exported_symbol,_{library}_entry"]
                else:
                    link = ["-shared", "-Wl,--gc-sections", f"-Wl,-Map=lib{library}.la.map",
                            f"-Wl,--version-script={FIXTURE.resolve()}/lib{library}.ver"]
                subprocess.run(["c++", *link, "common.o", f"{library}.o",
                                "-o", f"lib{library}.so", *libs],
                               cwd=source, check=True, env=env)
            link = (["-Wl,-dead_strip", "-Wl,-map,tool.map"] if sys.platform == "darwin"
                    else ["-Wl,--gc-sections", "-Wl,-Map=tool.map"])
            subprocess.run(["c++", *cxxflags, "-w", "-I../src", *glib, "tool.cpp",
                            "../src/common.o", *link, "-o", "tool", *libs],
                           cwd=reach / "utils", check=True, env=env)

        def generate(self, work, families):
            program = (
                "import importlib.util, json, sys\n"
                "from pathlib import Path\n"
                "spec = importlib.util.spec_from_file_location('glib_call_sites', sys.argv[1])\n"
                "driver = importlib.util.module_from_spec(spec)\n"
                "sys.modules['glib_call_sites'] = driver\n"
                "spec.loader.exec_module(driver)\n"
                "table = driver.read_families(Path(sys.argv[3]))\n"
                "rows, missing = driver.generate(Path(sys.argv[2]), ['bdb'], table)\n"
                "print(json.dumps([driver.render(driver.build_rows(rows, table)), missing]))\n")
            return subprocess.run(
                ["uv", "run", "--no-project", "--managed-python", "--with", driver.CLANG_REQUIREMENT,
                 "python", "-X", "lazy_imports=normal", "-c", program,
                 str(Path(__file__).with_name("glib_call_sites.py")), str(work), str(families)],
                capture_output=True, text=True, timeout=300, check=False)

        def test_the_fixture_is_reproduced_and_the_table_is_closed(self):
            with tempfile.TemporaryDirectory() as temporary:
                work = Path(temporary)
                self.build(work)
                result = self.generate(work, FIXTURE / "families.tsv")
                self.assertEqual(result.returncode, 0, result.stderr)
                text, missing = json.loads(result.stdout)
                self.assertEqual(sorted(text.splitlines()), sorted(EXPECTED))
                self.assertEqual(missing, UNPLACED)
                shorter = work / "shorter.tsv"
                shorter.write_text("".join(
                    line for line in (FIXTURE / "families.tsv").read_text().splitlines(True)
                    if not line.startswith("strcmp")))
                result = self.generate(work, shorter)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("1 calls to functions outside the table: strcmp (libc)", result.stderr)


if __name__ == "__main__":
    unittest.main()
