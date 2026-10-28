"""The sentence-tie fixture regeneration of the contract driver (#1175).

Read the declared residual rows, run the pin over every tie input and rewrite the
fixture. regenerate_sentence_ties takes the driver's Kit as a keyword-only parameter;
contract-diff.py keeps a same-signature adapter for its --regenerate-sentence-ties mode.
"""
import ctypes as C
import hashlib
from pathlib import Path
import resource
import tempfile
import time

from contract_abi import B, P, S, U, Z, final_step_sentences
from contract_case_helpers import candidate_rows


def declared_sentence_residuals():
    """Read membership from the frozen gate's §12 declaration, not a copy."""
    root = Path(__file__).resolve().parents[2]
    doc = (root / 'docs/findings/sentence-surface.md').read_text()
    section = doc.split('\n## 12. ', 1)[1].split('\n## 13.', 1)[0]
    declaration = section.split(
        'Only the following six shared-comparison rows are class (a), as ruled.', 1)[1]
    table = declaration.split('| Input | Pin deciding f32 bits | Subject native costs |\n', 1)[1]
    names = set()
    for line in table.splitlines()[1:]:
        if not line.startswith('|'):
            break
        name = line.split('|')[1].strip().strip('`')
        assert name not in names, 'duplicate declared residual'
        names.add(name)
    assert len(names) == 6, 'frozen declared residual count moved'
    return names


def encode_sentence_tie(row, guessed, sentences, nbest, residual_sentences, residual_nbest):
    """Pool each text once per input; all displayed rows have NBEST type 1."""
    texts = list(dict.fromkeys(sentences + residual_sentences))
    ids = {text: index for index, text in enumerate(texts)}
    def indexed(items):
        """Encode texts as comma-separated IDs from the enclosing row's shared pool."""
        return ','.join(str(ids[text]) for text in items)
    def displayed(items):
        """Encode displayed (index, text) pairs as index:pooled-ID entries in row order."""
        return ','.join(f'{index}:{ids[text]}' for index, text in items)
    return '\t'.join(row[:3] + [str(int(guessed)), '\x01'.join(texts),
                                indexed(sentences), displayed(nbest),
                                indexed(residual_sentences), displayed(residual_nbest)])


def regenerate_sentence_ties(prefix, fixture, *, Kit):
    """Refresh pin expectations; preserve the six approved exact subject rows.

    The first three columns are historical native-tail selection metadata.
    Text pooling keeps indexed duplicates and complete NBEST rows compact.
    Membership comes only from the frozen §12 table; ordinary cases cannot
    carry subject expectations. No observations or other outputs are saved.
    """
    suspend_offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)

    def fields(path):
        """Read key=value lines, ignoring empty, comment and separator-free lines.

        Split only the first equals sign and let later duplicate keys replace earlier
        values; propagate file-read errors.
        """
        return dict(line.split('=', 1) for line in path.read_text().splitlines()
                    if line and not line.startswith('#') and '=' in line)

    manifest = fields(prefix / 'oracle-pin.txt')
    source_pin = fields(Path(__file__).resolve().parents[2] / 'tools/oracle/oracle-pin.txt')
    expected = (f"libpinyin-{source_pin['libpinyin_tag']}-{source_pin['libpinyin_commit']}"
                f"+model20-{source_pin['model_archive_sha256']}+dbm-bdb")
    assert manifest['pin_ref'] == expected and manifest['dbm'] == 'BerkeleyDB', 'off-pin bdb prefix'
    so = prefix / 'lib/libpinyin.so'
    assert hashlib.sha256(so.read_bytes()).hexdigest() == manifest['shared_object_sha256'], \
        'pin library checksum differs'
    verified_files = 0
    for name, field in (('oracle-data.sha256', 'data_manifest_sha256'),
                        ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256')):
        raw = (prefix / name).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == manifest[field], 'pin data manifest checksum differs'
        for line in raw.decode().splitlines():
            digest, relative = line.split('  ', 1)
            assert hashlib.sha256((prefix / relative).read_bytes()).hexdigest() == digest, \
                'pin data checksum differs: ' + relative
            verified_files += 1
    print(f'pin verified: library and {verified_files} data files', flush=True)

    raw = fixture.read_text()
    headers = [line for line in raw.splitlines() if line.startswith('#')]
    assert '# format=sentence-tie-order-v1' in headers, 'unsupported tie fixture format'
    cases = [line.split('\t') for line in raw.splitlines() if not line.startswith('#')]
    assert cases and all(len(row) == 9 and int(row[1]) > 0 for row in cases), 'bad tie fixture'
    assert len({row[0] for row in cases}) == len(cases), 'duplicate selected input'
    declared = declared_sentence_residuals()
    recorded = {row[0] for row in cases if row[7] or row[8]}
    assert recorded == declared, 'subject expectations must match the frozen declared membership exactly'
    out = []
    row_count = 0
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    with tempfile.TemporaryDirectory(prefix='sentence-ties-') as scratch:
        k = Kit('pinyin', str(so.resolve()), str(prefix / 'lib/libpinyin/data'), scratch)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
        try:
            for ordinal, row in enumerate(cases, 1):
                text = row[0]
                pool = row[4].split('\x01')
                residual_sentences = [pool[int(index)] for index in row[7].split(',')] if row[7] else []
                residual_nbest = [(int(index), pool[int(text_id)])
                                  for index, text_id in (item.split(':') for item in row[8].split(','))] if row[8] else []
                assert k.fn('reset', B, P)(k.inst)
                k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, text.encode())
                guessed = k.fn('guess_sentence', B, P)(k.inst)
                assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0x1e)
                nbest = []
                for index, candidate in enumerate(candidate_rows(k, k.inst)):
                    assert candidate is not None, 'unreadable candidate for ' + repr(text)
                    if candidate[0] == 1:
                        pointer, rank = P(), C.c_ubyte(0xff)
                        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(
                            k.inst, index, C.byref(pointer))
                        assert k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
                            k.inst, pointer, C.byref(rank))
                        nbest.append((rank.value, candidate[1]))
                sentences = final_step_sentences(k, k.inst)
                out.append(encode_sentence_tie(row, guessed, sentences, nbest,
                                              residual_sentences, residual_nbest))
                row_count += len(nbest)
                if ordinal % 1000 == 0:
                    print(f'pin expectations: {ordinal}/{len(cases)}', flush=True)
        finally:
            if k._inst:
                k.fn('free_instance', None, P)(k._inst)
            if k._ctx:
                k.fn('fini', None, P)(k._ctx)
    headers = [line for line in headers if not line.startswith('# pin_ref=')]
    headers.insert(1, '# pin_ref=' + manifest['pin_ref'])
    elapsed_suspend = (time.clock_gettime(time.CLOCK_BOOTTIME) -
                       time.clock_gettime(time.CLOCK_MONOTONIC) - suspend_offset)
    assert abs(elapsed_suspend) < 0.01, 'discard: suspend overlapped pin regeneration'
    fixture.write_text('\n'.join(headers + out) + '\n')
    print(f'regenerated {fixture}: {len(cases)} distinct inputs, '
          f'{sum(int(row[1]) for row in cases)} corpus entries, {row_count} ordered NBEST rows; '
          f'pin-equal expectations {len(cases) - len(declared)}, exact residual expectations {len(declared)}', flush=True)
    print(f'suspend overlap: none (clock delta {elapsed_suspend:.6f} seconds)', flush=True)
