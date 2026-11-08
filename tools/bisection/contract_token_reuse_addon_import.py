"""The BDB-only token reuse cases (inventory row 45, #1175, #762).

They have no frozen reference yet. The family's other cases are Rust tests
(`token_reuse_addon_import` in the capi crates).
"""
from contract_case_helpers import USER_ALL


def register(case, *, token_reuse_sequence):
    """Register these cases at their original driver position."""
    # name: steps. The no-mask control holds on the parent too.
    TOKEN_REUSE_SEQUENCES = {
        'no-mask': [('add', 0), ('add', 1), ('save',), ('add', 2), ('save',)],
        'mask-all': [('add', 0), ('save',), ('mask',) + USER_ALL, ('save',), ('add', 1), ('save',)],
    }
    # A user bigram row under the token and another that lists it as a successor:
    # the pin's `mask_out` drops both, so the reused token starts clean.
    TOKEN_REUSE_BIGRAM_ROWS = [
        (b'\x01\x00\x00\x07', b''.join(n.to_bytes(4, 'little') for n in (100, 0x01000001, 100))),
        (b'\x01\x00\x00\x01', b''.join(n.to_bytes(4, 'little') for n in (100, 0x07000001, 100))),
    ]
    for _mode in ('pinyin', 'zhuyin'):
        # BDB only: token_reuse_files compares the saved bigram's non-empty rows
        # through db_dump; kc/tkrzw report only its presence, which cannot check
        # that the seeded stale successor was removed.
        case('issue762-token-reuse-stale-bigram-%s' % _mode, mode=_mode, cells=('bdb',), issues=('762',))(
            lambda k: token_reuse_sequence(k, TOKEN_REUSE_SEQUENCES['mask-all'], TOKEN_REUSE_BIGRAM_ROWS))
        # The no-mask control needs the same BDB row comparison to establish
        # that the seeded bigram survives; kc/tkrzw presence alone cannot do so.
        case('issue762-token-reuse-stale-bigram-no-mask-%s' % _mode, mode=_mode, control=True, cells=('bdb',), issues=('762',))(
            lambda k: token_reuse_sequence(k, TOKEN_REUSE_SEQUENCES['no-mask'], TOKEN_REUSE_BIGRAM_ROWS))
