#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Source regressions for the in-tree policy generator and measurement reader.

Synthetic nm, Callgrind, smaps and linker records exercise parsing, scoping,
normalization and reconciliation. They need no compiler, profiler or worker.
"""

import hashlib
import io
from pathlib import Path
import re
import runpy
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock


HERE = Path(__file__).resolve().parent
GENERATOR = runpy.run_path(str(HERE / "generate-function-order.py"))
MEASUREMENT = runpy.run_path(str(HERE / "measure-function-order.py"))
TEARDOWN = runpy.run_path(str(HERE / "bisection/teardown.py"))
LIBRARY = Path("/synthetic/libpinyin.so")
HEADER = "oxpinyin-function-order-v1\tlibpinyin.so.15\n"


def row(name, section, address="0000000000000000"):
    return f"{name}|{address}| t | FUNC |0000000000000010| |{section}\n"


def profile(body, total):
    return f"version: 1\npositions: instr line\nevents: Ir\n{body}totals: {total}\n"


class MeasurementTests(unittest.TestCase):
    def test_unordered_arm_is_refused_before_any_measurement(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'lib').mkdir()
            for facade in ('pinyin', 'zhuyin'):
                (root / 'lib' / f'lib{facade}.so.15.0.0').write_bytes(b'not an ordered ELF')
            for profile in ('phrases', 'journals'):
                (root / profile).mkdir()
                (root / profile / 'profile.manifest').write_text('')
            argv = ['measure-function-order.py']
            for arm in ('base', 'pr', 'control', 'pin'):
                argv += ['--subject', f'{arm}={root}']
            for option in ('bisect', 'system-dir', 'profiles-dir', 'work-dir'):
                argv += ['--' + option, str(root)]
            argv += ['--cpu', '0', '--system-identity', hashlib.sha256(b'').hexdigest()]
            with mock.patch.object(sys, 'argv', argv), \
                    mock.patch.dict(MEASUREMENT['main'].__globals__, run=lambda *a: (b'ext4', b'')), \
                    mock.patch.dict(MEASUREMENT['GEN'], manifest=lambda *a: '',
                                    collection_scratch=mock.Mock(side_effect=AssertionError('measurement started'))):
                with self.assertRaisesRegex(ValueError, 'function ordering: arm base/pinyin refused'):
                    MEASUREMENT['main']()

    def setUp(self):
        # These are 4 KiB Linux ELF fixtures, independent of the test host's
        # native page size (macOS arm64 uses 16 KiB).
        pages = mock.patch.dict(MEASUREMENT["executed_profile"].__globals__, PAGE=4096)
        pages.start()
        self.addCleanup(pages.stop)

    def test_residency_below_pin_and_the_exact_upper_boundary_meet_target(self):
        meets = MEASUREMENT["meets_residency_target"]
        self.assertTrue(meets(356, 452, 0))
        self.assertTrue(meets(368, 436, 0))
        self.assertTrue(meets(500, 452, 48))
        self.assertFalse(meets(501, 452, 0))
        self.assertFalse(meets(356, 452, 49))

    def test_instruction_positions_include_call_row_bases_and_cross_page_bytes(self):
        text = profile(
            "ob=(1) /synthetic/libpinyin.so\nfn=(1) f\n0xfff 1 2\n"
            "cob=(2) /foreign.so\ncfn=(2) target\ncalls=1 0x50 2\n"
            "0x1001 2 999\n+1 * 3\nob=(2)\nfn=(3) foreign\n0x80 4 5\n", 10)
        row = MEASUREMENT["executed_profile"](text.encode(), LIBRARY, {0xfff: 2, 0x1002: 1})
        self.assertEqual(row, {"ir": 10, "pages": {0, 1}, "bytes": 3})

    def test_executed_addresses_must_match_original_elf(self):
        text = profile("ob=/synthetic/libpinyin.so\nfn=f\n0x123 1 2\n", 2)
        with self.assertRaisesRegex(ValueError, "does not match ELF"):
            MEASUREMENT["executed_profile"](text.encode(), LIBRARY, {0x124: 1})

    def test_only_executable_mappings_of_the_selected_inode_count(self):
        text = ("1000-4000 r-xp 0000 08:01 42 /selected.so\nRss: 8 kB\nPss: 6 kB\n"
                "4000-8000 r--p 3000 08:01 42 /selected.so\nRss: 4 kB\nPss: 4 kB\n"
                "8000-9000 r-xp 0000 08:01 43 /foreign.so\nRss: 4 kB\nPss: 4 kB\n")
        self.assertEqual(MEASUREMENT["text_memory"](text, 42), {"text_pages": 2, "text_pss_kib": 6})


class GeneratorTests(unittest.TestCase):
    def parse(self, text):
        return GENERATOR["callgrind_functions"](io.StringIO(text), LIBRARY)

    def test_exclusive_costs_compressed_names_and_exact_object(self):
        text = profile(
            "ob=(1) /synthetic/libpinyin.so\nfn=(1) ancestor\ncfn=(2) callee\n"
            "calls=1 0x10 1\n0x1 1 90\n"
            "fn=(2)\n0x10 1 7\n+1 * 2\n"
            "ob=(2) /synthetic/libpinyin.so.extra\nfn=(3) foreign\n0x20 2 4\n"
            "ob=(1)\nfn=(4) not_executed\n0x30 3 0\n", 13)
        self.assertEqual(self.parse(text), {"callee"})

    def test_missing_or_inconsistent_totals_are_errors(self):
        prefix = "positions: instr\nevents: Ir\nob=/synthetic/libpinyin.so\nfn=f\n0x1 2\n"
        for text in (prefix, prefix + "totals: 3\n", prefix + "calls=1 0x2\ntotals: 2\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                self.parse(text)

    def test_ir_event_column_and_omitted_trailing_zero(self):
        text = ("positions: instr\nevents: Dr Ir Dw\nob=/synthetic/libpinyin.so\n"
                "fn=f\n0x1 8 2\nfn=zero_ir\n0x2 3\ntotals: 11 2 0\n")
        self.assertEqual(self.parse(text), {"f"})

    def test_unresolved_compression_and_wrong_object_fail(self):
        for body in ("ob=(9)\n", "ob=/synthetic/other.so\nfn=f\n0x1 1 2\n"):
            with self.subTest(body=body), self.assertRaises(ValueError):
                self.parse(profile(body, 2))

    def test_policy_order_is_lexical_with_init_precedence(self):
        render = GENERATOR["policy"]
        first = render("pinyin", {"z", "a"}, {"b", "z"})
        second = render("pinyin", set(reversed(["a", "z"])), {"z", "b"})
        self.assertEqual(first, HEADER + "init\ta\ninit\tz\nsteady\tb\n")
        self.assertEqual(first, second)

    def test_paths_preserve_generics_and_closures(self):
        name = "<crate::Store<T>>::open::{closure#1}"
        self.assertEqual(GENERATOR["canonical"](name + "::h0123456789abcdef.llvm.abcdef"), name)
        self.assertNotEqual(name, "<crate::Store<U>>::open::{closure#1}")

    def test_instantiating_crate_disambiguates_without_hash_identity(self):
        name = "crate::open::<T>"
        identity = GENERATOR["stable_identity"]
        first = identity(name, "_RIfakeECsOLD_16oxpinyin_runtime")
        self.assertEqual(first, identity(name, "_RIfakeECsNEW_16oxpinyin_runtime"))
        self.assertEqual(first, name + " [instantiating crate: oxpinyin_runtime]")
        self.assertNotEqual(first, identity(name, "_RIfakeECsNEW_15oxpinyin_facade"))

    def test_both_facades_and_profiles_are_mandatory(self):
        with self.assertRaises(ValueError):
            GENERATOR["pairs"](["pinyin=one"], ("pinyin", "zhuyin"), "--library")
        with self.assertRaises(ValueError):
            GENERATOR["pairs"](["pinyin:phrases=one"],
                               tuple(f"{f}:{p}" for f in ("pinyin", "zhuyin")
                                     for p in ("phrases", "journals")), "--init")

    def test_exact_unnamed_crt_start_resolves_without_address_guessing(self):
        reader = GENERATOR["library_symbols"]
        data = row("deregister_tm_clones", ".text", "00000000000647e0")
        with mock.patch.dict(reader.__globals__, run=lambda _: data):
            table = reader(LIBRARY)
        self.assertEqual(table["0x00000000000647e0"], ("deregister_tm_clones", ".text"))
        self.assertNotIn("0x00000000000647e1", table)

    def test_unknown_or_ambiguous_measured_paths_fail(self):
        measured = GENERATOR["measured_paths"]
        table = {"_Rraw": ("crate::open", ".text")}
        with mock.patch.dict(measured.__globals__, callgrind_functions=lambda *_: {"_Rraw"}):
            for available in ({}, {"crate::open": {".text.one", ".text.two"}}):
                with self.subTest(available=available), self.assertRaises(ValueError):
                    measured(Path("-"), LIBRARY, table, available)

    def init_array_fixture(self, values, relocations=(), symbols=()):
        size = 8 * len(values)
        section = ("  [20] .init_array       INIT_ARRAY      "
                   f"0000000000002000 002000 {size:06x} 08 WAo  0   0  8\n")
        words = [f"{value.to_bytes(8, 'little').hex()[:8]} "
                 f"{value.to_bytes(8, 'little').hex()[8:]}" for value in values]
        dump = ("\nHex dump of section '.init_array':\n"
                "  0x0000000000002000 " + " ".join(words) + " ................\n\n")
        relocs = ("Relocation section '.rela.dyn' at offset 0x1 contains entries:\n"
                  "    Offset             Info             Type               "
                  "Symbol's Value  Symbol's Name + Addend\n"
                  + "".join(f"{offset:016x}  0000000000000008 R_X86_64_RELATIVE"
                            f"                         {addend:x}\n"
                            for offset, addend in relocations))
        nm = "".join(f"{address:016x} T {name}\n" for address, name in symbols)

        def dispatch(command):
            if command[:2] == ["readelf", "-SW"]:
                return section
            if command[:2] == ["readelf", "-x"]:
                return dump
            if command[:2] == ["readelf", "-rW"]:
                return relocs
            return nm

        return dispatch

    def test_init_array_symbols_use_exact_constructor_targets(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x1000, 0x2000], symbols=[(0x1000, "first_ctor"), (0x2000, "second_ctor")])
        with mock.patch.dict(reader.__globals__, run=run):
            self.assertEqual(reader(LIBRARY), ["first_ctor", "second_ctor"])

    def test_init_array_nonzero_unresolved_slot_raises(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x4000], symbols=[(0x1000, "first_ctor")])
        with mock.patch.dict(reader.__globals__, run=run), self.assertRaisesRegex(ValueError, "0x4000"):
            reader(LIBRARY)

    def test_init_array_zero_slot_without_relocation_is_exempt(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x0, 0x1000], symbols=[(0x1000, "first_ctor")])
        with mock.patch.dict(reader.__globals__, run=run):
            self.assertEqual(reader(LIBRARY), ["first_ctor"])

    def test_init_array_relocated_zero_slot_resolves_from_addend(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x0], relocations=[(0x2000, 0x2000)],
                                      symbols=[(0x2000, "relocated_ctor")])
        with mock.patch.dict(reader.__globals__, run=run):
            self.assertEqual(reader(LIBRARY), ["relocated_ctor"])

    def test_init_array_relocated_target_missing_raises(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x0], relocations=[(0x2000, 0x3000)],
                                      symbols=[(0x1000, "first_ctor")])
        with mock.patch.dict(reader.__globals__, run=run), self.assertRaisesRegex(ValueError, "0x3000"):
            reader(LIBRARY)

    def test_capture_timeout_is_a_value_error(self):
        capture = GENERATOR["capture"]
        subprocess = GENERATOR["subprocess"]
        with mock.patch.object(subprocess, "run",
                               side_effect=subprocess.TimeoutExpired(["worker"], 1800)) as run:
            with self.assertRaisesRegex(ValueError, "measurement child timed out: worker"):
                capture(["worker"], {})
        self.assertEqual(run.call_args.kwargs["timeout"], 1800)

    def test_teardown_exports_keep_only_the_selected_facade(self):
        reader = GENERATOR["teardown_exports"]
        with mock.patch.dict(reader.__globals__, run=lambda _: (
                "00000001 T pinyin_init@@LIBPINYIN\n"
                "00000002 T zhuyin_init@@LIBZHUYIN\n"
                "00000003 T g_free\n")):
            self.assertEqual(reader(LIBRARY, "pinyin"), "pinyin_init")
        with self.assertRaises(ValueError):
            with mock.patch.dict(reader.__globals__, run=lambda _: "00000001 T g_free\n"):
                reader(LIBRARY, "pinyin")

    def test_first_candidate_toggles_list_every_facade_export(self):
        toggles = GENERATOR["first_candidate_toggles"]
        names = GENERATOR["FIRST_CANDIDATE"]["zhuyin"]
        exports = ",".join(["zhuyin_init", "pinyin_guess_candidates",
                            *(f"zhuyin_{name}" for name in names)])
        self.assertEqual(toggles("zhuyin", exports), [f"zhuyin_{name}" for name in names])
        with self.assertRaisesRegex(ValueError, "lacks the first-candidate exports "
                                                "zhuyin_guess_candidates_after_cursor$"):
            toggles("zhuyin", exports.replace(",zhuyin_guess_candidates_after_cursor", ""))

    def test_first_candidate_matches_the_worker_path_to_m1(self):
        """set_options, alloc_instance, td_first and td_read_candidate in teardown.h."""
        source = (HERE / "bisection/teardown.h").read_text()

        def body(signature):
            start = source.index("{", source.index(signature))
            depth = 0
            for end in range(start, len(source)):
                depth += {"{": 1, "}": -1}.get(source[end], 0)
                if not depth:
                    return source[start:end]
            raise AssertionError(signature)

        resolve = body("static void td_resolve(")
        common, branches = resolve.split("if (api->zhuyin) {")
        zhuyin, pinyin = branches.split("} else {")
        pattern = re.compile(r'TD_RESOLVE\((\w+), "(\w+)"\)|api->(\w+)\) = td_symbol'
                             r'\(library, facade, "(\w+)"\)')

        def resolved(text):
            return {(field or other): (suffix or suffix2)
                    for field, suffix, other, suffix2 in pattern.findall(text)}

        scope = {"pinyin": resolved(common) | resolved(pinyin),
                 "zhuyin": resolved(common) | resolved(zhuyin)}
        calls = set(re.findall(r"api->(?:common\.)?(\w+)\(",
                               body("static bool td_first(") + body("static bool td_read_candidate(")))
        run = body("static int run_teardown_mode(")
        path = run[run.index("api.common.init("):run.index('td_checkpoint("M1"')]
        for field in ("set_options", "alloc_instance"):
            self.assertIn(f"api.common.{field}(", path)
        for facade, fields in scope.items():
            expected = {"set_options", "alloc_instance"} | {
                fields[field] for field in calls if field in fields}
            self.assertEqual(set(GENERATOR["FIRST_CANDIDATE"][facade]), expected, facade)
            self.assertEqual(len(GENERATOR["FIRST_CANDIDATE"][facade]), len(expected), facade)

    def test_teardown_command_has_the_worker_boundary_arguments(self):
        args = SimpleNamespace(bisect=Path("/bin/bisect"), system_dir=Path("/data"))
        command = GENERATOR["teardown_command"](args, "pinyin", Path("/lib.so"), Path("/user"))
        self.assertEqual(command, [str(args.bisect), "--teardown", "pinyin", "/lib.so",
                                   "/data", "/user", "time", "0", "1", "1",
                                   "first-candidate", "-"])


class ProfileManifestTests(unittest.TestCase):
    """The generator reads the manifests the teardown provisioner writes (section 7.4)."""

    class Reached(Exception):
        pass

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.system = self.root / "data"
        self.system.mkdir()
        (self.system / "table.conf").write_bytes(b"conf\n")
        self.profiles = self.root / "profiles"
        for name in TEARDOWN["PROFILES"]:
            user = self.profiles / name / "user"
            (user / "nested" / "deeper").mkdir(parents=True)
            (user / "a.txt").write_bytes(name.encode())
            (user / "nested" / "b.bin").write_bytes(b"\x00\x01")
            (user / "nested" / "deeper" / "c").write_bytes(b"")
            (user / "nested.txt").write_bytes(b"sibling")
            self.write_teardown_manifest(user)

    @staticmethod
    def write_teardown_manifest(user):
        # teardown.py's provision() format, spelled out independently of the
        # shared implementation: size<TAB>sha256<TAB>relative path.
        rows = ["oxpinyin-startup-profile-v1"]
        for path in sorted(user.rglob("*")):
            if path.is_file():
                data = path.read_bytes()
                rows.append(f"{len(data)}\t{hashlib.sha256(data).hexdigest()}\t{path.relative_to(user)}")
        (user.parent / "profile.manifest").write_text("\n".join(rows) + "\n")

    def collect(self):
        collect = GENERATOR["collect_policies"]
        args = SimpleNamespace(
            bisect=Path("/bin/bisect"), system_dir=self.system, profiles_dir=self.profiles,
            work_dir=self.root, cpu=0, init=[], steady=[],
            system_identity=GENERATOR["STARTUP_PROFILE"]["identity"](self.system))

        def reached(_root):
            raise self.Reached

        with mock.patch.dict(collect.__globals__, collection_scratch=reached):
            collect(args, {}, {})

    def rejected(self, message):
        with self.assertRaisesRegex(ValueError, message):
            self.collect()

    def test_teardown_format_with_nested_directories_is_accepted(self):
        user = self.profiles / "phrases" / "user"
        self.assertEqual((user.parent / "profile.manifest").read_text(), GENERATOR["manifest"](user))
        with self.assertRaises(self.Reached):
            self.collect()
        # The provisioner's own existing-tree check accepts the same files.
        self.assertEqual(set(TEARDOWN["provision"](None, None, self.profiles)), set(TEARDOWN["PROFILES"]))

    def test_measurement_and_provisioner_write_the_same_manifest(self):
        user = self.profiles / "journals" / "user"
        recorded = (user.parent / "profile.manifest").read_text()
        self.assertEqual(MEASUREMENT["GEN"]["manifest"](user), recorded)
        self.assertEqual(TEARDOWN["STARTUP_PROFILE"].manifest(user), recorded)

    def test_changed_file_is_rejected(self):
        (self.profiles / "phrases" / "user" / "nested" / "b.bin").write_bytes(b"\x00\x02")
        self.rejected(r"differs from .*phrases/user: changed nested/b\.bin$")

    def test_added_file_is_rejected(self):
        (self.profiles / "journals" / "user" / "nested" / "deeper" / "new").write_bytes(b"x")
        self.rejected(r"differs from .*journals/user: added nested/deeper/new$")

    def test_removed_file_is_rejected(self):
        (self.profiles / "phrases" / "user" / "a.txt").unlink()
        self.rejected(r"differs from .*phrases/user: removed a\.txt$")

    def test_symlink_is_rejected(self):
        (self.profiles / "phrases" / "user" / "link").symlink_to("a.txt")
        self.rejected(r"manifest input contains a symlink: .*phrases/user/link$")

    def test_tab_or_newline_in_a_path_is_rejected(self):
        for name in ("new\nline", "tab\tname"):
            with self.subTest(name=name):
                user = self.root / "control" / name.encode().hex()
                user.mkdir(parents=True)
                try:
                    (user / name).write_bytes(b"x")
                except OSError as error:
                    self.skipTest(f"file system refuses {name!r}: {error}")
                with self.assertRaisesRegex(ValueError, "path contains a tab or newline: " + re.escape(repr(name))):
                    GENERATOR["manifest"](user)

    def test_old_four_column_manifest_is_rejected(self):
        manifest = self.profiles / "phrases" / "profile.manifest"
        lines = manifest.read_text().split("\n")
        manifest.write_text("\n".join([lines[0], *("file\t" + line for line in lines[1:-1]), ""]))
        self.rejected(r"line 2 is not size<TAB>sha256<TAB>path: .*phrases/profile\.manifest$")

    def test_identity_of_a_fixed_directory(self):
        # SHA-256 of the header plus a.txt, nested/b.bin, nested/deeper/c and
        # nested.txt rows, in that order, computed outside Python.
        self.assertEqual(GENERATOR["STARTUP_PROFILE"]["identity"](self.profiles / "empty" / "user"),
                         "dc34a1df1c7365ad0dbdc19b51a9a7c20d64a6f0954877e90c16feac9ddef08f")


if __name__ == "__main__":
    unittest.main()
