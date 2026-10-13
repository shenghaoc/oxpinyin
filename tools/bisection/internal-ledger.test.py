#!/usr/bin/env python3
"""Compiler-backed regression checks for J identity and ledger sensitivity."""

import importlib.util
import hashlib
import os
import signal
import subprocess
from collections import defaultdict
from types import SimpleNamespace
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from clang_identity import Clang
from coverage_tiers import lexical_mask, rust_items, cited_items
from render_ledger import align, annotate_comparison
import gcov_measure
import compiler_cells
import finish_ledger

spec = importlib.util.spec_from_file_location("internal_ledger", Path(__file__).with_name("internal-ledger.py"))
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


class IdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.pin = Path(cls.temp.name)
        cls.source = cls.pin / "probe.cpp"
        cls.source.write_text("""
namespace pinyin {
static int helper(int value) { return value + 1; }
static double helper(double value) { return value + 2.0; }
}
extern void receive(int (*callback)(int));
struct Base { virtual ~Base() {} virtual int go() const = 0; };
struct One : Base { ~One() override {} int go() const override { return 1; } };
struct Two : Base { ~Two() override {} int go() const override { return 2; } };
template<unsigned N> int templated(int v) { return v + N; }
template<unsigned N> struct Box { int member(int v) { return N + v; } double member(double v) { return N + v; } };
extern "C" double entry(Base *base) {
    receive(pinyin::helper);
    return pinyin::helper(1.0) + base->go();
}
extern "C" int pinned(One *one) { return one->go(); }
extern "C" void deleted(One *one) { delete one; }
extern "C" int instance() { return templated<2>(1); }
extern "C" int member_instance() { Box<2> value; return value.member(1); }
extern "C" int actual(int value) { return value; }
extern "C" int alias_entry(int value) __attribute__((alias("actual")));
extern "C" int call_alias(int value) { return alias_entry(value); }
""")
        cls.ast, cls.ast_edges, cls.concrete, cls.overrides, _, cls.metadata = Clang().extract(cls.source, ["-std=c++17"], cls.pin)
        cls.bc = cls.pin / "probe.bc"
        ledger.run(["clang++", "-std=c++17", "-O0", "-g", "-fno-inline", "-emit-llvm", "-c", cls.source, "-o", cls.bc])
        cls.nodes, cls.symbols, cls.edges, cls.external, cls.indirect = ledger.llvm(cls.pin, cls.bc, cls.ast)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_overloads_have_distinct_usr_and_signature(self):
        helpers = [n for n in self.ast.values() if n["name"] == "helper"]
        self.assertEqual(len(helpers), 2)
        self.assertEqual(len({n["usr"] for n in helpers}), 2)
        self.assertEqual(len({n["signature"] for n in helpers}), 2)
        self.assertTrue(all(n["usr"] in self.nodes for n in helpers))

    def test_callback_and_direct_overload_are_both_reachable(self):
        paths = ledger.witness(self.nodes, self.edges, {"entry": self.symbols["entry"]})
        helpers = [n for n in self.ast.values() if n["name"] == "helper"]
        self.assertTrue(all(n["usr"] in paths for n in helpers))
        kinds = {kind for a, b, kind in self.edges if a == self.symbols["entry"]}
        self.assertIn("LLVM-direct", kinds)
        self.assertIn("LLVM-address/callback", kinds)
        self.assertIn("entry", ledger.format_path(self.nodes, paths[self.symbols["entry"]][1]))

    def test_virtual_hierarchy_preserves_pure_base_and_overrides(self):
        methods = [n for n in self.ast.values() if n["name"] == "go"]
        self.assertEqual(len(methods), 3)
        base = next(n["usr"] for n in methods if "Base::" in n["signature"])
        one = next(n["usr"] for n in methods if "One::" in n["signature"])
        two = next(n["usr"] for n in methods if "Two::" in n["signature"])
        self.assertNotIn(base, self.concrete)
        self.assertIn(one, self.concrete)
        self.assertIn(two, self.concrete)
        self.assertEqual({b for a, b in self.overrides if a == base}, {one, two})
        self.assertTrue(any(a == self.symbols["entry"] and b == base and kind == "virtual" for a, b, kind, line in self.ast_edges))
        self.assertTrue(any(a == self.symbols["pinned"] and b == one and kind == "virtual" for a, b, kind, line in self.ast_edges))
        self.assertTrue(any(a == self.symbols["entry"] for a, line in self.indirect))

    def test_unknown_library_body_stays_a_boundary(self):
        self.assertIn((self.symbols["entry"], "_Z7receivePFiiE"), self.external)
        self.assertNotIn("_Z7receivePFiiE", self.symbols)

    def test_static_receiver_and_deleting_destructor_are_recorded(self):
        one = next(n for n in self.ast.values() if n['name'] == 'go' and 'One::' in n['signature'])
        self.assertTrue(any(a == self.symbols['pinned'] and receiver == one['owner_usr'] for a, line, target, receiver in self.metadata['virtual_sites']))
        self.assertTrue(any(a == self.symbols['deleted'] and receiver == one['owner_usr'] for a, line, target, receiver in self.metadata['deletes']))

    def test_function_body_and_compiler_template_origin(self):
        helper = next(n for n in self.ast.values() if n['name'] == 'helper' and n['canonical_type'] == 'int (int)')
        self.assertLessEqual(helper['line'], helper['body_start'])
        self.assertLessEqual(helper['body_start'], helper['body_end'])
        generic = next(n['usr'] for n in self.ast.values() if n['function_kind'] == 30 and n['name'] == 'templated')
        self.assertTrue(any(n.get('template_usr') == generic for n in self.ast.values()))

    def test_class_template_member_origin_does_not_conflate_same_line_overloads(self):
        primary = [n for n in self.ast.values() if n['name'] == 'member' and n.get('definition')]
        self.assertEqual(len(primary), 2)
        integer = next(n['usr'] for n in primary if n['canonical_type'] == 'int (int)')
        floating = next(n['usr'] for n in primary if n['canonical_type'] == 'double (double)')
        instantiated = [n for n in self.nodes.values() if n.get('template_usr') == integer]
        self.assertTrue(instantiated)
        self.assertFalse(any(n.get('template_usr') == floating for n in self.nodes.values()))

    def test_exact_llvm_alias_reaches_its_emitted_body(self):
        self.assertEqual(self.symbols['alias_entry'], self.symbols['actual'])
        self.assertIn((self.symbols['call_alias'], self.symbols['actual'], 'LLVM-direct'), self.edges)
        self.assertNotIn((self.symbols['call_alias'], 'alias_entry'), self.external)


class TierTests(unittest.TestCase):
    def test_abort_shim_flushes_counters_and_retains_signal(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'abort.cpp'
            source.write_text('#include <cstdlib>\nvoid target() { std::abort(); }\nint main() { target(); }\n')
            shim = root / 'shim.cpp'
            shim.write_text(gcov_measure.FLUSH_SHIM)
            obj, shim_obj, exe = root / 'abort.cpp.o', root / 'shim.o', root / 'abort'
            gcov_measure.command(['g++', '-g', '-O0', '--coverage', '-c', source, '-o', obj])
            gcov_measure.command(['g++', '-c', shim, '-o', shim_obj])
            gcov_measure.command(['g++', '--coverage', obj, shim_obj, '-o', exe])
            import resource
            def no_core(): resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
            result = subprocess.run([exe], capture_output=True, timeout=10, preexec_fn=no_core)
            self.assertEqual(result.returncode, -signal.SIGABRT)
            self.assertIn(b'J-GCOV-ABORT-BEGIN\n', result.stdout)
            self.assertIn(b'J-GCOV-ABORT-END\n', result.stdout)
            counts = gcov_measure.counters(root, root)
            self.assertTrue(any(identity[0] == '_Z6targetv' and count > 0 for identity, count in counts.items()))

    def test_reviewed_operator_join_requires_extent_signature_and_unit(self):
        identity = ('_Zeq', 'src/storage/chewing_key.h', 81, 91, 'pinyin.cpp')
        node = dict(file=identity[1], line=81, body_end=91,
                    signature='operator==(_ChewingKey, _ChewingKey)',
                    llvm_bindings={('_Zeq11_ChewingKeyS_', 'pinyin.cpp')})
        matched, unknown = align({'actual': node}, {identity: {'MATCH'}}, ledger.symbol_base)
        self.assertEqual(set(matched), {'actual'})
        self.assertFalse(unknown)
        without_extent = dict(node)
        without_extent.pop('body_end')
        self.assertEqual(set(align({'actual': without_extent}, {identity: {'MATCH'}}, ledger.symbol_base)[0]), {'actual'})
        for changes in ({'body_end': 90}, {'signature': 'operator==(int, int)'},
                        {'llvm_bindings': {('_Zeq11_ChewingKeyS_', 'zhuyin.cpp')}},
                        {'llvm_bindings': {('_Zeq5WrongS_', 'pinyin.cpp')}}):
            matched, unknown = align({'actual': dict(node, **changes)}, {identity: {'MATCH'}}, ledger.symbol_base)
            self.assertFalse(matched)
            self.assertEqual(unknown, {identity})

    def test_citations_in_strings_or_test_items_do_not_classify_shipping_items(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'lib.rs'
            source.write_text('''const EXAMPLE: &str = r#"// pin.cpp:2-4"#;
/// implements real (pin.cpp:5-7)
pub fn real() { /* pin.cpp:8-9 */ }
#[cfg(test)] mod tests { /// pin.cpp:10-11
fn fake() {} }
#[test] fn other() {}
''')
            mask, comments = lexical_mask(source.read_text())
            self.assertNotIn('pin.cpp:2-4', '\n'.join(c[2] for c in comments))
            items = list(rust_items(source, root))
            self.assertEqual({item['name'] for item in items}, {'real'})

    def test_complete_callback_type_and_binding_count_are_required(self):
        node = {'file': 'external-header/stl_algo.h', 'signature': 'run(bool (*)(pinyin::Record const&, pinyin::Record const&))'}
        prototype = next(iter(ledger.callback_prototypes(node['signature'])))
        self.assertEqual(ledger.singleton_callback(node, {prototype: {'actual-target'}}, set(), True)[0], 'actual-target')
        self.assertIsNone(ledger.singleton_callback(node, {prototype: {'one', 'two'}}, set(), True))
        self.assertIsNone(ledger.singleton_callback(node, {prototype: {'one'}}, {'unknown-binding'}, True))
        self.assertIsNone(ledger.singleton_callback(node, {prototype: {'one'}}, set(), False))
        nodes = {'one': dict(pin=True, function_kind=8, canonical_type='bool(pinyin::Record const&,pinyin::Record const&)'),
                 'two': dict(pin=True, function_kind=21, is_static=True, canonical_type='bool(pinyin::Record const&,pinyin::Record const&)')}
        pool, unknown = ledger.internal_callback_pool(nodes, {('caller', key, 'LLVM-address/callback') for key in nodes})
        self.assertEqual(next(iter(pool.values())), {'one', 'two'})

    def test_gcov_execution_aligns_by_exact_symbol_extent_and_tu(self):
        with tempfile.TemporaryDirectory() as temp:
            pin = Path(temp)
            source = pin / 'probe.cpp'
            source.write_text('namespace pinyin { int same(int v) { return v; } double same(double v) { return v; } }\nint main() { return pinyin::same(0); }\n')
            ast, *_ = Clang().extract(source, ['-std=c++17'], pin)
            bc = pin / 'probe.bc'
            ledger.run(['clang++', '-O0', '-g', '-emit-llvm', '-c', source, '-o', bc])
            nodes, *_ = ledger.llvm(pin, bc, ast)
            exe = pin / 'probe'
            obj = pin / 'probe.cpp.o'
            gcov_measure.command(['g++', '-O0', '--coverage', '-c', source, '-o', obj])
            gcov_measure.command(['g++', '--coverage', obj, '-o', exe])
            gcov_measure.command([exe])
            counts = gcov_measure.counters(pin, pin)
            positive = {key: {'MATCH/probe'} for key, count in counts.items() if count > 0}
            matched, unknown = align(nodes, positive, ledger.symbol_base)
            integer = next(n['usr'] for n in ast.values() if n['name'] == 'same' and n['canonical_type'] == 'int (int)')
            floating = next(n['usr'] for n in ast.values() if n['name'] == 'same' and n['canonical_type'] == 'double (double)')
            self.assertIn(integer, matched)
            self.assertNotIn(floating, matched)
            identity = next(key for key in positive if key[0] == nodes[integer]['symbol'])
            wrong = (identity[0], 'other.cpp', *identity[2:])
            self.assertFalse(align(nodes, {wrong: {'MATCH'}}, ledger.symbol_base)[0])

    def test_contract_matcher_rejects_equal_fatal_failures(self):
        module = gcov_measure.contract_module()
        spec = dict(abort=module.NO_ABORT, either=False, crash=None, stderr=False)
        fatal = dict(exit=-6, result=None)
        self.assertFalse(module.matches(spec, fatal, fatal))

    def test_measurement_observer_preserves_abort_and_crash_arms(self):
        module = gcov_measure.contract_module()
        spec = dict(abort=False, either=True, crash={'ret': False}, stderr=True, userdir=False)
        reference = dict(exit=-6, result=None, stderr='reference assert pathname')
        measurement = dict(exit=-6, result=None, stderr='measurement assert pathname')
        self.assertTrue(gcov_measure.same_reference(module, spec, reference, measurement))
        reference['exit'] = measurement['exit'] = -11
        self.assertFalse(gcov_measure.same_reference(module, spec, reference, measurement))


class MutationTests(unittest.TestCase):
    def test_exact_pinyin_deletion_does_not_use_zhuyin_overload(self):
        nodes = {
            "pinyin-root": {"file": "src/pinyin.cpp", "line": 326, "name": "pinyin_init"},
            "pinyin-format": {"file": "src/pinyin.cpp", "line": 172, "name": "check_format"},
            "zhuyin-format": {"file": "src/zhuyin.cpp", "line": 126, "name": "check_format"},
        }
        rows = {k: "a distinct ledger row for " + k for k in nodes}
        baseline = dict(rows)
        result = ledger.mutation_check(rows, nodes)
        self.assertEqual(len(result), 2)
        self.assertEqual(rows, baseline)
        incomplete = {k: v for k, v in rows.items() if k != "pinyin-format"}
        ok, missing, extra = ledger.check_rows(rows, incomplete)
        self.assertFalse(ok)
        self.assertEqual(missing, {"pinyin-format"})
        self.assertFalse(extra)

    def test_extra_row_is_not_complete(self):
        self.assertFalse(ledger.check_rows({"root": "row"}, {"root": "row", "invented": "row"})[0])


class ReviewGuardTests(unittest.TestCase):
    def test_partial_citation_exclusion_is_identity_and_source_specific(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); pin = root / 'pin'
            src = pin / 'src/storage/phrase_index.cpp'
            src.parent.mkdir(parents=True); src.write_text('configured pin source')
            rust = root / 'crates/oxpinyin-user/src/store.rs'
            rust.parent.mkdir(parents=True); rust.write_text('shipping item')
            key = 'c:@N@pinyin@S@SubPhraseIndex@F@add_unigram_frequency#i#i#'
            nodes = {key: dict(file='src/storage/phrase_index.cpp', name='add_unigram_frequency',
                               usr=key, body_start=150, body_end=178)}
            item = dict(file='crates/oxpinyin-user/src/store.rs', name='add_unigram_frequency',
                        line=562, end=580, comment_line=552,
                        comment='SubPhraseIndex::add_unigram_frequency (phrase_index.cpp:150-178)')
            with patch('coverage_tiers.shipping_crates', return_value={rust.parent.parent}), \
                 patch('coverage_tiers.rust_items', return_value=[item]):
                self.assertFalse(cited_items(root, pin, nodes, tightened='role'))
            alternate = dict(item, name='whole_port',
                             comment='Implements SubPhraseIndex::add_unigram_frequency (phrase_index.cpp:150-178)')
            with patch('coverage_tiers.shipping_crates', return_value={rust.parent.parent}), \
                 patch('coverage_tiers.rust_items', return_value=[alternate]):
                self.assertIn(key, cited_items(root, pin, nodes, tightened='role'))

    def test_rebased_worker_preserves_extra_environment_and_timeout(self):
        module = gcov_measure.contract_module()
        with tempfile.TemporaryDirectory() as scratch:
            result = SimpleNamespace(returncode=0, stdout=b'{"ret": true}\n', stderr=b'')
            with patch.object(module.subprocess, 'run', return_value=result) as run:
                observed = module.run_worker('pinyin', Path('/p'), Path('/data'), 'probe', scratch,
                                             extra_env={'J_TEST_ENV': 'retained'}, timeout=60)
            self.assertEqual(run.call_args.kwargs['env']['J_TEST_ENV'], 'retained')
            self.assertEqual(run.call_args.kwargs['timeout'], 60)
            self.assertEqual(observed['result'], {'ret': True})

    def test_default_sweep_matches_ordinary_driver_scope(self):
        good = dict(exit=0, result={}, stderr='', userfiles=[], measurement_abort_flush=False)
        cases = {name: dict(cells=['bdb'], mode='pinyin') for name in ('ordinary', 'axis-c-owned')}
        module = SimpleNamespace(CELL='bdb', CASES=cases, __file__='driver', run_worker=lambda *a, **kw:dict(good),
                                 matches=lambda *a:True)
        with patch.object(gcov_measure, 'verify_prefix'), patch.object(gcov_measure, 'contract_module', return_value=module), \
             patch.object(gcov_measure, 'clear'), patch.object(gcov_measure, 'counters', return_value={}), \
             patch.object(gcov_measure, 'same_reference', return_value=True):
            result = gcov_measure.contracts(Path('/unused'), Path('/p'), Path('/z'), Path('/out'), Path('/pin'))
        self.assertEqual(result[3], [('ordinary', 'MATCH')])

    def test_resume_replays_only_invalid_cases_and_preserves_completed_credit(self):
        behavior = defaultdict(set, valid={'contract/complete'}, stale={'contract/paused'},
                               shared={'contract/complete', 'contract/paused'})
        previous = (behavior, {'old-bad'}, {'contract/paused'},
                    [('complete', 'MATCH'), ('paused', 'TIMEOUT')])
        fresh = (defaultdict(set, repaired={'contract/paused'}), set(), set(), [('paused', 'MATCH')])
        with patch.object(gcov_measure, 'contracts', return_value=fresh) as replay:
            result = gcov_measure.reconcile_interrupted(previous, ['paused'], *([Path('/unused')] * 5))
        self.assertEqual(replay.call_args.kwargs['names'], {'paused'})
        self.assertEqual(dict(result[0]), {'valid': {'contract/complete'},
                                        'shared': {'contract/complete'},
                                        'repaired': {'contract/paused'}})
        self.assertFalse(result[1]); self.assertFalse(result[2])
        self.assertEqual(result[3], [('complete', 'MATCH'), ('paused', 'MATCH')])
        with self.assertRaisesRegex(RuntimeError, 'identities absent'):
            gcov_measure.reconcile_interrupted(previous, ['unknown'], *([Path('/unused')] * 5))

    def test_surface_shim_markers_are_validated_separately(self):
        row = b'["input", 1, true, true, [], []]\n'
        markers = b'J-GCOV-ABORT-BEGIN\nJ-GCOV-ABORT-END\n'
        records, aborts = gcov_measure.surface_records(markers + row)
        self.assertEqual(records, [['input', 1, True, True, [], []]])
        self.assertEqual(aborts, 1)
        for malformed in (b'unknown diagnostic\n' + row, b'J-GCOV-ABORT-BEGIN\n' + row,
                          b'J-GCOV-ABORT-END\n' + row, b'J-GCOV-ABORT-BEGIN\n',
                          b'[]\n', b'J-GCOV-ABORT-BEGIN\n' + markers):
            with self.assertRaises(RuntimeError): gcov_measure.surface_records(malformed)

    def test_each_oracle_binary_is_authenticated(self):
        with tempfile.TemporaryDirectory() as temp:
            prefix = Path(temp)
            (prefix / 'lib').mkdir()
            provenance = {}
            for name in ('libpinyin', 'libzhuyin'):
                body = ('pinned ' + name).encode()
                (prefix / f'lib/{name}.so').write_bytes(body)
                provenance[name] = hashlib.sha256(body).hexdigest()
            (prefix / 'oracle-pin.txt').write_text('shared_object_sha256=' + provenance['libpinyin'] + '\n')
            self.assertEqual(gcov_measure.oracle_hashes(prefix, provenance), provenance)
            for name in provenance:
                binary = prefix / f'lib/{name}.so'
                original = binary.read_bytes()
                binary.write_bytes(b'tampered')
                with self.assertRaisesRegex(RuntimeError, name + ' oracle SHA'):
                    gcov_measure.oracle_hashes(prefix, provenance)
                binary.write_bytes(original)

    def test_cell_is_bdb_before_fixture_registration(self):
        for inherited in ('kc', 'tkrzw'):
            with patch.dict(os.environ, CONTRACT_DIFF_CELL=inherited):
                module = gcov_measure.contract_module()
                self.assertEqual(module.CELL, 'bdb')
                self.assertEqual(os.environ['CONTRACT_DIFF_CELL'], 'bdb')
                with patch.object(module, '_craft_bdb') as bdb, patch.object(module, '_craft_kc') as kc, \
                     patch.object(module, '_craft_tkrzw') as tkrzw:
                    module._craft_db('probe', [('key', 'value')], 1)
                    bdb.assert_called_once_with('probe', [('key', 'value')], 1, keep=False)
                    kc.assert_not_called(); tkrzw.assert_not_called()

    def test_gate_rejects_zero_filtered_failed_or_multiple_executions(self):
        name = gcov_measure.GATE_TESTS[0]
        valid = f'test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out;\n'
        self.assertEqual(gcov_measure.gate_summary(valid, name), 1)
        for bad in (valid.replace('1 passed', '0 passed'), valid.replace('1 passed', '3 passed'),
                    valid.replace('0 failed', '1 failed'), valid.replace('0 ignored', '1 ignored'),
                    valid.replace(name, 'different_test'), valid + valid):
            with self.assertRaises(RuntimeError): gcov_measure.gate_summary(bad, name)

    def test_all_three_contract_sides_timeout_without_credit(self):
        good = dict(exit=0, result={}, stderr='', userfiles=[], measurement_abort_flush=False)
        for timeout_side in range(3):
            calls = []
            def worker(*args, **kwargs):
                calls.append(kwargs['timeout'])
                if len(calls) - 1 == timeout_side:
                    raise subprocess.TimeoutExpired('probe', kwargs['timeout'])
                return dict(good)
            module = SimpleNamespace(CELL='bdb', CASES={'probe': dict(cells=['bdb'], mode='pinyin')},
                                     run_worker=worker, __file__='driver', observed=lambda x:x)
            with patch.object(gcov_measure, 'verify_prefix'), patch.object(gcov_measure, 'contract_module', return_value=module), \
                 patch.object(gcov_measure, 'clear'), patch.object(gcov_measure, 'counters', return_value={('body',): 1}), \
                 patch.object(gcov_measure, 'same_reference', return_value=True):
                result = gcov_measure.contracts(Path('/unused'), Path('/p'), Path('/z'), Path('/out'), Path('/pin'))
            self.assertEqual(calls, [60, 60, 60])
            self.assertFalse(result[0])
            self.assertIn('contract/probe', result[2])
            self.assertEqual(result[3], [('probe', 'TIMEOUT')])
            self.assertEqual(module.__file__, 'driver')

    def test_prefix_failure_prevents_driver_or_native_comparison(self):
        with patch.object(gcov_measure, 'verify_prefix', side_effect=RuntimeError('libzhuyin oracle SHA mismatch')), \
             patch.object(gcov_measure, 'contract_module') as driver:
            with self.assertRaisesRegex(RuntimeError, 'libzhuyin'):
                gcov_measure.contracts(Path('/unused'), Path('/p'), Path('/z'), Path('/out'), Path('/pin'))
            driver.assert_not_called()

    def test_gate_executes_each_exact_test_and_requires_its_summary(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp)
            deps = target / 'debug/deps'
            deps.mkdir(parents=True)
            binary = deps / 'sentence_surface_parity-probe'
            binary.touch(); binary.chmod(0o700)
            called = []
            def run(argv, **kwargs):
                called.append(argv)
                name = argv[2]
                return SimpleNamespace(returncode=0, stdout=(f'test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out;\n').encode())
            with patch.object(gcov_measure.subprocess, 'run', side_effect=run):
                self.assertIn('four tests PASS', gcov_measure.gates(target, Path('/export'), Path('/model')))
            self.assertEqual([argv[2] for argv in called], list(gcov_measure.GATE_TESTS))
            self.assertTrue(all(argv[1:] == ['--exact', argv[2], '--include-ignored', '--nocapture'] for argv in called))

    def test_missing_synthetic_descriptor_rejects_present_body(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            (out / 'probe.cpp.o').touch()
            nodes = {f'k{i}': dict(file='src/probe.cpp', llvm_bindings={(f's{i}', 'probe.cpp')}) for i in range(15)}
            reviews = {key: ('ABSENT_INLINE', [], 'review') for key in nodes}
            bodies = '\n'.join(f'0 T s{i}' for i in range(15))
            counters = '\n'.join(f'0 d __gcov_.s{i}' for i in range(15))
            with patch.object(finish_ledger, 'IDENTITY_REVIEWS', reviews), patch.object(gcov_measure, 'command', return_value=bodies):
                with self.assertRaisesRegex(RuntimeError, 'gcov descriptor missing'):
                    finish_ledger.verify_implicit_objects(nodes, out, lambda x:x)
            with patch.object(finish_ledger, 'IDENTITY_REVIEWS', reviews), patch.object(gcov_measure, 'command', return_value=bodies + '\n' + counters):
                self.assertEqual(len(finish_ledger.verify_implicit_objects(nodes, out, lambda x:x)), 15)

    def test_same_identity_corrupted_compiler_cells_are_rejected(self):
        nodes = {'root': dict(file='src/pinyin.cpp', line=1, signature='root()', usr='root', name='root'),
                 'helper': dict(file='src/helper.cpp', line=2, signature='helper(int)', usr='helper', name='helper')}
        paths = {'root': ('entry', [('root', 'export')]),
                 'helper': ('entry', [('root', 'export'), ('helper', 'LLVM-direct')])}
        state = dict(nodes=nodes, paths=paths, direct_paths=paths, pinned_paths={}, fields=[], ast={},
                     sites=[('helper', 3)], ambiguous=[], resolved_sites=[], singleton_callbacks=[], external={('root', 'outside')})
        rows = [compiler_cells.function_cells(ledger, state, key) + ['T5', '', 'manual'] for key in nodes]
        rows += compiler_cells.boundary_cells(ledger, state)
        self.assertEqual(compiler_cells.validate(ledger, state, nodes, rows), 4)
        for row_index in range(len(rows)):
            columns = [0, 1, 2, 4, 5] + ([8] if row_index >= 2 else [])
            for column in columns:
                changed = [list(row) for row in rows]
                changed[row_index][column] += ' corrupted'
                self.assertEqual(changed[row_index][3], rows[row_index][3])
                with self.assertRaisesRegex(RuntimeError, 'compiler-derived'):
                    compiler_cells.validate(ledger, state, nodes, changed)

    def test_row_comparison_reports_all_changed_fields(self):
        old = '| function/direct | pin | f(int) | usr | root | path | T5 | old MATCH | reason |\n'
        new = '| function/direct | new pin | f(double) | usr | root | new path | BEHAVIOR-COVERED | new MATCH | new reason |\n'
        text = annotate_comparison(new, old, '2da3f089')
        for column in ('pin source', 'signature', 'path/resolution', 'classification',
                       'credited differential', 'counterpart/reason'):
            self.assertIn(column + ' previously:', text)
        self.assertNotIn('identity previously:', text)
        with self.assertRaisesRegex(RuntimeError, 'removed rows'):
            annotate_comparison('', old, '2da3f089')


if __name__ == "__main__":
    unittest.main()
