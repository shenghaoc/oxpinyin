#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for the ABI helpers the contract-diff cases share (#1175).

The helpers drive a Kit through `fn`, `text`, `alloc` and `inst`, so a stand-in
Kit whose `fn` hands back Python functions exercises each one without a
library. An out-parameter is written through `ref._obj`, the object `byref`
wraps."""

import ctypes as C
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import contract_abi as abi


class FakeKit:
    mode = 'pinyin'
    inst = 'cached-instance'

    def __init__(self, **functions):
        self.functions = functions
        self.asked = []
        self.allocated = 0
        self.freed = []

    def fn(self, name, res, *args):
        self.asked.append((name, res, args))
        return self.functions[name]

    def alloc(self):
        self.allocated += 1
        return 'new-instance-%d' % self.allocated

    def text(self, pointer):
        """Kit.text: None for NULL, else the decoded string, whose pointer is freed."""
        if not pointer:
            return None
        self.freed.append(pointer)
        return 'text@%d' % pointer


def candidate_facade(rows):
    """The four list accessors over `rows`, a list of (type, text); a candidate pointer is its index + 1."""
    def get_n_candidate(inst, count):
        count._obj.value = len(rows)
        return True

    def get_candidate(inst, index, candidate):
        candidate._obj.value = index + 1
        return True

    def get_candidate_type(inst, candidate, kind):
        kind._obj.value = rows[candidate.value - 1][0]
        return True

    def get_candidate_string(inst, candidate, text):
        text._obj.value = rows[candidate.value - 1][1].encode()
        return True

    return dict(get_n_candidate=get_n_candidate, get_candidate=get_candidate,
                get_candidate_type=get_candidate_type, get_candidate_string=get_candidate_string)


class VocabularyTests(unittest.TestCase):
    def test_aliases_are_the_ctypes_types_the_cases_sign_with(self):
        self.assertEqual((abi.P, abi.U, abi.I, abi.S, abi.B, abi.Z),
                         (C.c_void_p, C.c_uint, C.c_int, C.c_char_p, C.c_bool, C.c_size_t))
        self.assertEqual(abi.UNTOUCHED, 0xABCDEF)

    def test_array_view_matches_the_public_garray_prefix(self):
        self.assertEqual([(name, kind) for name, kind in abi.Arr._fields_], [('data', C.c_void_p), ('len', C.c_uint)])


class ParseTests(unittest.TestCase):
    def test_consumed_parses_on_a_new_instance_or_the_cached_one(self):
        kit = FakeKit(parse_more_full_pinyins=lambda inst, text: len(text) if inst.startswith('new') else -1)
        self.assertEqual(abi.consumed(kit, 'parse_more_full_pinyins', b'nihao'), 5)
        self.assertEqual(kit.asked[-1], ('parse_more_full_pinyins', abi.Z, (abi.P, abi.S)))
        self.assertEqual(abi.consumed(kit, 'parse_more_full_pinyins', b'nihao', fresh=False), -1)
        self.assertEqual(kit.allocated, 1)

    def test_key_out_keeps_the_untouched_sentinel_when_the_parser_writes_nothing(self):
        def writes(inst, text, key):
            key._obj.value = 0x1234
            return True

        kit = FakeKit(parse_full_pinyin=writes)
        self.assertEqual(abi.key_out(kit, b'ni'), [True, 0x1234])
        kit = FakeKit(parse_full_pinyin=lambda inst, text, key: False)
        self.assertEqual(abi.key_out(kit, b'zzz'), [False, abi.UNTOUCHED & 0xFFFF])

    def test_instance_builders_ignore_the_consumed_count_and_return_the_instance(self):
        kit = FakeKit(parse_more_full_pinyins=lambda inst, text: 0, parse_more_chewings=lambda inst, text: 0,
                      guess_sentence=lambda inst: True, guess_candidates=lambda inst, offset, sort: True)
        self.assertEqual(abi.full_inst(kit, b'nihao'), 'new-instance-1')
        self.assertEqual(abi.zhuyin_chewing(kit), 'new-instance-2')
        self.assertEqual(kit.asked[-1][0], 'parse_more_chewings')
        calls = []
        kit = FakeKit(parse_more_full_pinyins=lambda inst, text: calls.append(('parse', inst, text)),
                      guess_sentence=lambda inst: calls.append(('sentence', inst)),
                      guess_candidates=lambda inst, offset, sort: calls.append(('candidates', inst, offset, sort)))
        self.assertEqual(abi.guessed(kit), 'new-instance-1')
        self.assertEqual(calls, [('parse', 'new-instance-1', b'nihao'), ('sentence', 'new-instance-1'),
                                 ('candidates', 'new-instance-1', 0, 0)])


class SentenceTests(unittest.TestCase):
    def reader(self, outcome):
        def get_sentence(*args):
            out = args[-1]
            if outcome != 'untouched':
                out._obj.value = outcome
            return True
        return get_sentence

    def test_pinyin_sentence_distinguishes_untouched_null_and_text(self):
        for outcome, expected in (('untouched', 'untouched'), (0, None), (7, 'text@7')):
            with self.subTest(outcome=outcome):
                kit = FakeKit(get_sentence=self.reader(outcome))
                self.assertEqual(abi.sentence_out(kit, 'inst', 2), [True, expected])
                self.assertEqual(kit.asked[-1], ('get_sentence', abi.B, (abi.P, C.c_ubyte, C.POINTER(abi.P))))

    def test_zhuyin_sentence_has_no_index_argument(self):
        for outcome, expected in (('untouched', 'untouched'), (0, None), (9, 'text@9')):
            with self.subTest(outcome=outcome):
                kit = FakeKit(get_sentence=self.reader(outcome))
                self.assertEqual(abi.zhuyin_sentence_out(kit, 'inst'), [True, expected])
                self.assertEqual(kit.asked[-1], ('get_sentence', abi.B, (abi.P, C.POINTER(abi.P))))

    def test_owned_text_never_frees_the_untouched_seed(self):
        kit = FakeKit()
        self.assertEqual(abi.owned_text(kit, abi.P(abi.UNTOUCHED)), 'untouched')
        self.assertEqual(abi.owned_text(kit, abi.P(0)), None)
        self.assertEqual(abi.owned_text(kit, abi.P(5)), 'text@5')
        self.assertEqual(kit.freed, [5])

    def test_final_step_sentences_read_zhuyin_once(self):
        for functions, expected in ((self.reader(3), ['text@3']), (self.reader(0), [None]),
                                    (lambda inst, out: False, [])):
            with self.subTest(expected=expected):
                kit = FakeKit(get_sentence=functions)
                kit.mode = 'zhuyin'
                self.assertEqual(abi.final_step_sentences(kit, 'inst'), expected)
                self.assertEqual(len(kit.asked), 1)

    def test_final_step_sentences_probe_each_pinyin_rank_in_a_fork_and_stop_at_the_first_false(self):
        def ranks(inst, index, out):
            out._obj.value = 10 + index
            return index < 2

        self.assertEqual(abi.final_step_sentences(FakeKit(get_sentence=ranks), 'inst'), ['text@10', 'text@11'])
        self.assertEqual(abi.final_step_sentences(FakeKit(get_sentence=lambda inst, index, out: False), 'inst'), [])

    def test_a_probe_that_fails_for_another_reason_is_an_error(self):
        def broken(inst, index, out):
            raise RuntimeError('probe')

        with self.assertRaisesRegex(AssertionError, r'sentence probe failed \(exit code 1\)'):
            abi.final_step_sentences(FakeKit(get_sentence=broken), 'inst')


class CandidateTests(unittest.TestCase):
    ROWS = [(1, 'a'), (2, 'b'), (2, 'c')]

    def test_row_of_returns_the_first_borrowed_pointer_of_a_type(self):
        kit = FakeKit(**candidate_facade(self.ROWS))
        self.assertEqual(abi.row_of(kit, 'inst', 2).value, 2)
        self.assertEqual(abi.row_of(kit, 'inst', 1).value, 1)
        self.assertIsNone(abi.row_of(kit, 'inst', 3))

    def test_row_with_text_needs_the_type_and_the_text(self):
        kit = FakeKit(**candidate_facade(self.ROWS))
        self.assertEqual(abi.row_with_text(kit, 'inst', 2, 'c').value, 3)
        self.assertIsNone(abi.row_with_text(kit, 'inst', 1, 'c'))
        self.assertIsNone(abi.row_with_text(kit, 'inst', 2, 'z'))

    def test_choose_text_regenerates_the_list_at_the_offset_and_selects_the_match(self):
        chosen = []
        guessed = []
        facade = candidate_facade(self.ROWS)
        facade.update(guess_candidates=lambda inst, offset, sort: guessed.append((inst, offset, sort)),
                      choose_candidate=lambda inst, offset, candidate: chosen.append((inst, offset, candidate.value)) or 'done')
        kit = FakeKit(**facade)
        self.assertEqual(abi.choose_text(kit, 'inst', 4, 'c'), 'done')
        self.assertEqual(guessed, [('inst', 4, 0)])
        self.assertEqual(chosen, [('inst', 4, 3)])
        self.assertIsNone(abi.choose_text(kit, 'inst', 0, 'missing'))
        self.assertEqual(len(chosen), 1)


class TokenTests(unittest.TestCase):
    def test_unigram_of_reports_the_flag_and_keeps_the_untouched_seed(self):
        def writes(inst, token, freq):
            freq._obj.value = 77
            return True

        self.assertEqual(abi.unigram_of(FakeKit(token_get_unigram_frequency=writes), 5), [True, 77])
        self.assertEqual(abi.unigram_of(FakeKit(token_get_unigram_frequency=lambda inst, token, freq: False), 5),
                         [False, abi.UNTOUCHED])


class RestLayoutTests(unittest.TestCase):
    def test_rest_mirrors_two_unsigned_16_bit_offsets(self):
        self.assertEqual([(name, kind) for name, kind in abi.Rest._fields_],
                         [('begin', abi.C.c_ushort), ('end', abi.C.c_ushort)])
        self.assertEqual(abi.C.sizeof(abi.Rest), 4)
        rest = abi.Rest(3, 7)
        self.assertEqual((rest.begin, rest.end), (3, 7))

if __name__ == '__main__':
    unittest.main()
