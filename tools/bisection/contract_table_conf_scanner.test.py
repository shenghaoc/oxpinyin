#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the table.conf and scanner cases (#1175)."""
from pathlib import Path
import sys

import collections
import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_table_conf_scanner as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('hash-path/system-directory-pinyin', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('1119',)}),
            ('hash-path/user-directory-pinyin', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('1119',)}),
            ('hash-path/system-filename-pinyin', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('1119',)}),
            ('hash-path/user-filename-pinyin', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('1119',)}),
            ('hash-path/system-directory-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('1119',)}),
            ('hash-path/user-directory-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('1119',)}),
            ('hash-path/user-filename-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('1119',)}),
            ('table-conf-1378-unigram-lambda-one', (), {'mode': 'pinyin', 'control': False, 'stderr': True, 'issues': ('1378',)}),
            ('table-conf-1378-unigram-lambda-one-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'stderr': True, 'issues': ('1378',)}),
            ('table-conf-1378-unigram-lambda-zero-control', (), {'mode': 'pinyin', 'control': True, 'stderr': True, 'issues': ('1378',)}),
            ('table-conf-row-removed-gbk', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-row-removed-gbk-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-row-removed-gb', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-row-removed-gb-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-not-used-opengram', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-not-used-opengram-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-not-used-merged', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-not-used-merged-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-not-used-user-dictionary', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-not-used-user-dictionary-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-not-used-network-dictionary', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-not-used-network-dictionary-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-user-files-renamed', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-user-files-renamed-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-tsi-is-gb', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-later-row-wins', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-network-as-system-file', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-network-as-system-file-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-reserved-as-system-file', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-zero', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-zero-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-half', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-half-zhuyin', (), {'mode': 'zhuyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-one', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-one-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-negative-half', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-negative-half-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-negative-two', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-negative-two-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-one-and-a-half', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-one-and-a-half-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-two-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-million-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-database-format-other', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-database-format-other-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-versions-other', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-versions-other-zhuyin', (), {'mode': 'zhuyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-source-format-zhuyin', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-short-row-dropped', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-extra-word-dropped', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-addon-index-wraps', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-addon-moved', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-addon-files-swapped', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-addon-not-used', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-addon-index-zero', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-source-format-unknown', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-source-format-unknown-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-database-format-unknown', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-database-format-unknown-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-default-name-unknown', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-default-name-unknown-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-file-type-unknown', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-file-type-unknown-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-addon-index-16', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-addon-index-16-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('user-conf-abort-database-format-unknown', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'issues': ('694', '525')}),
            ('user-conf-abort-database-format-unknown-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'issues': ('694', '525')}),
            ('table-conf-abort-default-dictionary', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-default-dictionary-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-load-library-not-used', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': False, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-load-library-not-used-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': False, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-load-addon-system-file', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': False, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-source-format-line-missing', (), {'mode': 'pinyin', 'abort': False, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-abort-source-format-line-missing-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'userdir': True, 'control': False, 'issues': ('694', '525')}),
            ('table-conf-lambda-line-missing', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-exponent-without-digits', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-line-missing-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-exponent-without-digits-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-hex', (), {'mode': 'pinyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-lambda-hex-zhuyin', (), {'mode': 'zhuyin', 'control': True, 'issues': ('694', '525')}),
            ('table-conf-919-header-one-line', (), {'mode': 'pinyin', 'issues': ('919',), 'control': True}),
            ('table-conf-919-header-no-final-newline', (), {'mode': 'pinyin', 'issues': ('919',), 'control': True}),
            ('table-conf-919-word-nul-truncates', (), {'mode': 'pinyin', 'issues': ('919',), 'control': True}),
            ('table-conf-919-source-token-nul', (), {'mode': 'pinyin', 'issues': ('919',)}),
            ('table-conf-919-database-token-nul', (), {'mode': 'pinyin', 'issues': ('919',)}),
            ('table-conf-919-lambda-dot-only', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('919',), 'control': True}),
            ('table-conf-919-header-leading-space', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('919',), 'control': True}),
            ('table-conf-919-binary-version-line-missing', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('919',), 'control': True}),
            ('table-conf-919-model-version-line-missing', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('919',), 'control': True}),
            ('table-conf-abort-919-word-width-overlong', (), {'mode': 'pinyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('919',)}),
            ('table-conf-abort-919-word-width-overlong-zhuyin', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'userdir': True, 'control': False, 'issues': ('919',)}),
            ('table-conf-919-versions-positive-overflow', (), {'mode': 'pinyin', 'control': False, 'issues': ('919',)}),
            ('table-conf-919-versions-positive-overflow-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('919',)}),
            ('table-conf-919-versions-int-wrap', (), {'mode': 'pinyin', 'control': False, 'issues': ('919',)}),
            ('table-conf-919-versions-int-wrap-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('919',)}),
            ('table-conf-919-versions-negative', (), {'mode': 'pinyin', 'control': False, 'issues': ('919',)}),
            ('table-conf-919-versions-negative-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('919',)}),
            ('table-conf-919-versions-no-digits', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('919',), 'control': True}),
            ('table-conf-missing-with-dbms', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('545',)}),
            ('table-conf-missing-with-dbms-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('545',)}),
            ('table-conf-user-file-absolute', (), {'mode': 'pinyin', 'issues': ('694', '525')}),
            ('table-conf-user-file-absolute-zhuyin', (), {'mode': 'zhuyin', 'issues': ('694', '525')}),
            ('table-conf-addon-cleanup', (), {'mode': 'pinyin', 'stderr': True}),
            ('table-conf-addon-cleanup-zhuyin', (), {'mode': 'zhuyin', 'stderr': True}),
            ('table-conf-versions-negative-overflow', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-versions-negative-overflow-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-user-file-not-utf8', (), {'mode': 'pinyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-user-file-not-utf8-zhuyin', (), {'mode': 'zhuyin', 'control': False, 'issues': ('694', '525')}),
            ('table-conf-colliding-library-files', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-colliding-library-files-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-library-named-like-an-index', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-log-named-like-a-library', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-log-named-like-a-library-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-user-library-named-user-conf', (), {'mode': 'pinyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-user-library-named-user-conf-zhuyin', (), {'mode': 'zhuyin', 'stderr': True, 'issues': ('694', '525')}),
            ('table-conf-candidates-files-swapped', (), {'mode': 'pinyin', 'crash': {'ret': False, 'logs': [['libpinyin', 16]]}, 'issues': ('694', '525')}),
            ('table-conf-candidates-files-swapped-zhuyin', (), {'mode': 'zhuyin', 'crash': {'ret': False, 'logs': [['libzhuyin', 16]]}, 'issues': ('694', '525')})]
INJECTED = ('CASES', 'WORKER')


def stub(name):
    return collections.defaultdict(dict) if name == 'CASES' else Mock(name=name)


def declarations(state):
    return collect_declarations(family.register, registry=state.get('CASES'),
                                CASES=state['CASES'], WORKER=state['WORKER'])


class TableConfScannerTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
