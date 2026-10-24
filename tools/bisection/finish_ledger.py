"""Incremental J reconciliation; retain the authorized ledger, never captures.

Published MATCH rows are prior measured inputs, explicitly retained rather
than represented as a fresh full-suite run. Compiler extraction is independent
and current; every registered abort and the five new target controls replay.
The normal generator remains available to repeat every behavioral workload.
"""

from collections import Counter
import hashlib
import html
import importlib
import random
import re
from pathlib import Path

from coverage_reviews import IDENTITY_REVIEWS, REVIEWS
from render_ledger import align, anchors, baseline_rows, labels_text
import gcov_measure as measure

MANUAL_SEED = 8876703449563750359
MANUAL_POPULATION_SHA = '3371ea29dfd9875434722d5f7f9f87403b0bc7fadf2f9bcf78e9228816aa6694'
MANUAL_SAMPLE_SHA = 'c10a0b85a8139349dc3b12343756d0a3290e3f7914de441910b7f0a6a6f1c13c'
# Main's surrogate-segmentation and remember-phrase changes touch only the
# C-API files. All five source files for the exact sampled manual population
# and all synthetic lifetime targets remain unchanged; retain the sample.
MANUAL_CRATES_TREE = 'ecead320030bade4e7f72dacbe32ed5bc1edde3d'
POPULATION_MARK = 'manual-178 population'
SAMPLE_MARK = 'manual sample accepted (seed 8876703449563750359)'
UNKNOWN_MARK = 'unknown-24 population'
UNKNOWN_POPULATION_SHA = 'd68020f5642a5cd41b0e840782455d09dfd357a2495d4b392103bca95781ad3b'


def digest(keys):
    """Hash sorted newline-delimited identity strings, including a final newline."""
    return hashlib.sha256(('\n'.join(sorted(keys)) + '\n').encode()).hexdigest()


def table_rows(text):
    """Extract and HTML-unescape ledger table cells, omitting the header and separator."""
    return [[html.unescape(c.strip()) for c in line.strip('|').split('|')]
            for line in text.splitlines() if line.startswith('| ') and not line.startswith('| ---')
            and not line.startswith('| Kind |')]


def manual_sample(rows):
    """Recover the guarded manual population and deterministic twenty-identity sample.

    Prefer explicit population marks, falling back to T5-REVIEWED rows.
    Raise RuntimeError if the population count or either identity hash changes.
    """
    marked = [row[3] for row in rows if POPULATION_MARK in row[-1]]
    population = marked or [row[3] for row in rows if row[6] == 'T5-REVIEWED']
    if len(population) != 178 or digest(population) != MANUAL_POPULATION_SHA:
        raise RuntimeError('original manual-178 population changed; re-review required')
    sampled = set(random.Random(MANUAL_SEED).sample(sorted(population), 20))
    if digest(sampled) != MANUAL_SAMPLE_SHA:
        raise RuntimeError('manual sample identity set changed')
    return set(population), sampled


def source_mapping(ledger, key, node):
    """Return an explicit or registered source-review tier and mapping, or None.

    Resolve Rust anchors for registered reviews and classify bdb export absence
    as T2-UNREACHABLE.
    """
    if key in ledger.MAPPINGS:
        return 'T5-REVIEWED', ledger.counterpart(key)
    review = IDENTITY_REVIEWS.get(key, REVIEWS.get((node['file'], node['line'])))
    if review is None:
        return None
    status, targets, reason = review
    category = 'T2-UNREACHABLE' if status == 'ABSENT_BDB_EXPORTS' else 'T5-REVIEWED'
    return category, status + ' — ' + anchors(ledger.REPO, targets) + '; ' + reason


def verify_implicit_objects(nodes, out, symbol_base):
    """Cross-check exact synthetic ABI bodies/counter descriptors, not counts."""
    units = {}
    for obj in out.glob('*.cpp.o'):
        names = {line.split()[-1] for line in measure.command(['nm', '--defined-only', obj]).splitlines()
                 if len(line.split()) >= 3}
        bodies = {symbol_base(name) for name in names if not name.startswith('__gcov')}
        descriptors = {symbol_base(name.removeprefix('__gcov_.')) for name in names
                       if name.startswith('__gcov_.')}
        units[obj.name.removesuffix('.o').split('-')[-1]] = (bodies, descriptors)
    checked, descriptor_count = [], 0
    for key, review in IDENTITY_REVIEWS.items():
        if review[0] != 'ABSENT_INLINE': continue
        if key not in nodes: raise RuntimeError('reviewed implicit identity disappeared: ' + key)
        node = nodes[key]
        found, descriptor = False, False
        for symbol, unit in node.get('llvm_bindings', set()):
            # Artificial LLVM DISubprograms can omit DICompileUnit. A
            # cpp-local class still has one exact defining source TU; a
            # header's complete class/member ABI identity is the same ODR
            # body in every configured object. This check supplies only
            # source/object correspondence, never dynamic credit.
            if unit:
                candidates = [units.get(unit, (set(), set()))]
            elif node['file'].endswith('.cpp'):
                candidates = [units.get(Path(node['file']).name, (set(), set()))]
            else:
                candidates = units.values()
            for bodies, counters in candidates:
                found |= symbol in bodies
                descriptor |= symbol in counters
        if not found or not descriptor:
            raise RuntimeError('implicit ABI body or gcov descriptor missing; re-review needed: ' + key)
        descriptor_count += descriptor
        checked.append(key)
    if len(checked) != 15: raise RuntimeError('expected all fifteen reviewed synthetic lifetime bodies')
    print(f'Exact synthetic identities: 15 ABI bodies present; {descriptor_count} counter descriptors; public gcov has no aligned artificial-function records; no inferred execution credit', flush=True)
    return checked


def refresh(args, state):
    """Reconcile retained ledger evidence with fresh identities and targeted contract replays.

    Require guarded Rust-tree, population, and sample identities before
    retaining published MATCH evidence. Build measurement libraries and replay
    registered aborts/J controls; update or compare args.output and print
    results. Reject changed provenance, invalid flushes, or requested complete
    closure with remaining static or measurement limits.
    """
    ledger = importlib.import_module('internal-ledger')
    original = args.output.read_text()
    rows = table_rows(original)
    functions = {row[3]: row for row in rows if row[0].startswith('function/')}
    baseline = baseline_rows(original)
    nodes = state['nodes']
    inventory = dict(state['reachable'])
    inventory.update({key: nodes[key] for key in baseline if key not in inventory})
    if not ledger.check_rows(inventory, functions)[0]:
        raise RuntimeError('published compiler rows differ from fresh extraction')
    from compiler_cells import validate, function_cells, boundary_cells
    if args.check:
        validate(ledger, state, inventory, rows)
    else:
        # Regenerate all metadata, while this optional mode retains only
        # explicitly guarded behavioral columns from the published ledger.
        for key, row in functions.items():
            row[:6] = function_cells(ledger, state, key)
        prior = {(row[0], row[1], row[3]): row for row in rows
                 if not row[0].startswith('function/')}
        rebuilt = boundary_cells(ledger, state)
        for row in rebuilt:
            old_row = prior.get((row[0], row[1], row[3]))
            if old_row:
                row[7] = old_row[7]
                if row[0] == 'indirect': row[6] = old_row[6]
        rows = list(functions.values()) + rebuilt
    crates_tree = ledger.run(['git', '-C', ledger.REPO, 'rev-parse', 'HEAD:crates']).strip()
    if crates_tree != MANUAL_CRATES_TREE:
        raise RuntimeError('manual audit Rust tree changed; source re-review is required')
    if f'subject crates tree `{crates_tree}`' not in original:
        raise RuntimeError('retained MATCH rows belong to another Rust tree; repeat full generator')
    if ledger.run(['git', '-C', ledger.REPO, 'status', '--porcelain', '--', 'crates']).strip():
        raise RuntimeError('Rust source changed; retained footprints cannot be reused')
    population, sample = manual_sample(rows)
    unknown_population = {key for key, row in functions.items()
                          if row[6] == 'UNKNOWN-LIMIT' or UNKNOWN_MARK in row[-1]}
    if not unknown_population:
        # An initial refresh may predate population annotations. Recover only
        # the exact hash-guarded identities, never an inferred set of gaps.
        target_sites = {('src/lookup/phonetic_lookup.cpp', 172),
                        ('src/storage/zhuyin_parser2.cpp', 408),
                        ('src/storage/zhuyin_parser2.cpp', 718)}
        unknown_population = (set(IDENTITY_REVIEWS) | {
            key for key, n in nodes.items() if (n['file'], n['line']) in target_sites}) & set(functions)
    if len(unknown_population) != 24 or digest(unknown_population) != UNKNOWN_POPULATION_SHA:
        raise RuntimeError('original UNKNOWN population changed; reconcile its identities explicitly')
    for key in sample:
        if source_mapping(ledger, key, nodes[key]) is None:
            raise RuntimeError('sample has no accepted current source classification: ' + key)
    # The direct parser is also constructed on the stack by three bulk-text
    # loaders. Check their complete identities/call-address edges as well as
    # the context construction/assignment source review; never infer deadness
    # from counters or from absence of a Rust spelling.
    loaders = {key for key, n in nodes.items() if n['file'] in {
        'src/storage/chewing_large_table.cpp', 'src/storage/chewing_large_table2.cpp',
        'src/storage/phrase_index.cpp'} and n.get('name') == 'load_text'}
    if not loaders or loaders & set(state['paths']):
        raise RuntimeError('bulk text loader has an export witness; direct-parser absence needs review')
    if any(target in loaders and 'address' in kind for _, target, kind in state['edges']):
        raise RuntimeError('bulk text loader function address escapes; absence proof needs review')

    for name in ('measurement_build', 'measurement_config', 'pinyin_so', 'zhuyin_so'):
        value = getattr(args, name)
        if value is None:
            raise RuntimeError('--refresh-unknowns requires --' + name.replace('_', '-'))
        setattr(args, name, value.resolve())
    measure.build(args.pin, args.measurement_config, args.measurement_build, args.oracle)
    verify_implicit_objects(nodes, args.measurement_build, ledger.symbol_base)
    module = measure.contract_module()
    selected = [name for name, spec in module.CASES.items()
                if 'bdb' in spec['cells'] and (spec['abort'] is not module.NO_ABORT or name.startswith('j-'))]
    compiled_records, abort_receipts, counter_limits = set(), [], set()

    def receipt(name, reference, subject, measurement, counters, verdict):
        """Accumulate compiled identities and abort/counter-limit receipts from each replay.

        Print a limit when a measured SIGABRT lacks a completed counter flush.
        """
        compiled_records.update(counters)
        if measurement.get('counter_limit'): counter_limits.add(name)
        if reference['exit'] == -6:
            abort_receipts.append((name, measurement['exit'], measurement.get('measurement_abort_flush', False),
                                   not measurement.get('counter_limit', False), verdict))
        if measurement['exit'] == -6 and not measurement.get('measurement_abort_flush'):
            print('ABORT_FLUSH_LIMIT ' + name, flush=True)

    behavior, nonmatch, incomplete, verdicts = measure.contracts(
        args.oracle, args.pinyin_so, args.zhuyin_so, args.measurement_build, args.pin,
        progress=receipt, names=selected)
    bad = [(name, verdict) for name, verdict in verdicts if verdict != 'MATCH']
    if bad:
        raise RuntimeError('targeted reconciliation differential failures: ' + repr(bad))
    if any(exit_code != -6 or not completed for _, exit_code, completed, _, _ in abort_receipts):
        raise RuntimeError('SIGABRT flush failed or changed signal; counters do not establish nonexecution')
    synthetic_symbols = {symbol for key, review in IDENTITY_REVIEWS.items()
                         if review[0] == 'ABSENT_INLINE'
                         for symbol, _ in nodes[key].get('llvm_bindings', set())}
    if any(ledger.symbol_base(identity[0]) in synthetic_symbols for identity in compiled_records):
        raise RuntimeError('gcov now reports artificial members; re-align their records before claiming omission')
    executed, unmatched = align(nodes, behavior, ledger.symbol_base)
    for identity in sorted(unmatched):
        print('EXECUTED_GCC_ALIGNMENT_LIMIT ' + repr(identity), flush=True)
    resolved = Counter()
    for key, row in functions.items():
        origin = 'baseline-962' if key in baseline else 'added-current-analysis'
        old_category = row[6]
        reviewed = source_mapping(ledger, key, nodes[key])
        # Idempotent replay column: retain the previous other-workload labels,
        # and replace this refresh's labels rather than accumulating duplicates.
        prior = row[7].split('; J refresh: ', 1)[0]
        if key in executed:
            row[6] = 'BEHAVIOR-COVERED'
            labels = labels_text(executed[key])
            if prior.startswith('No credited execution') or prior == 'J refresh only':
                prior = 'J refresh only'
            row[7] = prior + '; J refresh: ' + labels
            mapping = ('Executed in matching differential(s); credit is confined to exercised inputs. '
                       'Registered abort footprints require completed __gcov_dump and unchanged -SIGABRT/reference fields.')
            if reviewed:
                mapping += ' Source-reviewed counterpart/absence: ' + reviewed[1]
            row[-1] = origin + '; ' + mapping
        elif old_category in {'UNKNOWN-LIMIT', 'NOT-EXECUTED-UNREVIEWED'} and reviewed:
            row[6], mapping = reviewed
            row[-1] = origin + '; ' + mapping
            row[7] = 'No execution credit; explicit source classification'
        if key in population and POPULATION_MARK not in row[-1]: row[-1] += '; ' + POPULATION_MARK
        if key in sample and SAMPLE_MARK not in row[-1]: row[-1] += '; ' + SAMPLE_MARK
        if key in unknown_population:
            if UNKNOWN_MARK not in row[-1]: row[-1] += '; ' + UNKNOWN_MARK
            resolved[row[6]] += 1
    categories = Counter(row[6] for row in functions.values())
    baseline_executed = sum(functions[key][6] == 'BEHAVIOR-COVERED' for key in baseline)
    # Existing unresolved-site rows are retained; coverage does not turn gcov
    # into a call-edge trace or silently resolve arbitrary library dispatch.
    for row in rows:
        if row[0] == 'indirect' and row[3] in executed:
            row[6] = 'caller executed MATCH; target unresolved'
            prior = row[7].split('; J refresh: ', 1)[0]
            row[7] = prior + '; J refresh: ' + labels_text(executed[row[3]])
    header = original.split('| Kind |', 1)[0]
    if '--output docs/findings/axis-j-bdb-ledger.md --refresh-unknowns' not in header:
        header = header.replace('--model "$MODEL20_DIR" --output docs/findings/axis-j-bdb-ledger.md',
                                '--model "$MODEL20_DIR" --output docs/findings/axis-j-bdb-ledger.md --refresh-unknowns')
    header = re.sub(r'Original compiler-identity baseline: \*\*962\*\* rows, of which executed MATCH \*\*\d+\*\*',
                    f'Original compiler-identity baseline: **962** rows, of which executed MATCH **{baseline_executed}**', header)
    counts = ', '.join(f'{name} **{count}**' for name, count in sorted(categories.items()))
    header = re.sub(r'Current classification: .*?\. No missing Rust counterpart',
                    'Current classification: ' + counts + '. No missing Rust counterpart', header)
    header = re.sub(r'The measurement orchestrator.*', '', header)
    header = header.replace('Fatal workers and failed forked sentence probes can lose counters.',
                            'The measurement-only SIGABRT handler dumps counters, restores default disposition and re-raises. Non-ABRT fatal workers and failed forked probes can still lose counters.')
    header = header.replace('**PARTIAL; #566 remains open.**',
                            '**Function-accounting reconciliation; static dispatch remains partial.**')
    header = header.replace('GCC executed records without an exact ledger alignment **5**; fatal measurement cases **347**.',
                            'Historical initial full-suite GCC alignment limits **5** and fatal measurement workers **347** (before the SIGABRT refresh below).')
    header = header.replace('Unique executed MATCH functions within baseline 962, per differential (overlap retained):',
                            'Historical initial full-workload executed MATCH functions within baseline 962, per differential (overlap retained):')
    header = header.replace('Contract cases **2325**: MATCH 2325.',
                            'Original completed contract workload **2325**: MATCH 2325; the targeted refresh below adds five controls and rechecks registered aborts.')
    marker = 'J reconciliation command and results:'
    if marker in header: header = header.split(marker, 1)[0]
    header += (marker + '\n\n'
               'The top command uses `--refresh-unknowns` to retain the published unchanged-workload MATCH rows, rebuild all compiler identities, and replay every bdb `abort=` case plus the five J controls. It requires only pin/config/oracle/build/output and measurement-build/config/pinyin-so/zhuyin-so arguments. Omit this flag to repeat all suites. Retained footprint inputs are guarded by the pin, independently rebuilt identity inventory and reviewed Rust crates tree; no capture is loaded.\n\n'
               f'Refresh: **{len(verdicts)} MATCH cases**, **{len(abort_receipts)} SIGABRT workers** with returned dump calls, unchanged SIGABRT and matching reference fields; **{sum(valid for _, _, _, valid, _ in abort_receipts)}** have valid decoded counters. **{len(incomplete)} residual limited workers**, of which **{len(counter_limits)}** counter-decode limits are the checksum-save tests that hard-limit file size to four bytes. Their limits remain intact and their counters receive no credit; a returned dump call alone does not establish nonexecution. Other fatal outcomes also remain limits. Of the prior 24 UNKNOWN rows: ' + ', '.join(f'{kind} **{count}**' for kind, count in sorted(resolved.items())) + '. '
               f'Current replay executed GCC alignment limits **{len(unmatched)}**; the initial full-suite unaligned count remains a historical measurement, not a closure proof. Synthetic bodies whose public gcov records are omitted are individually source-reviewed against exact object ABI/counter-descriptor identities and pin/Rust locations; they receive no inferred execution credit. [GCC gcov.cc:1764-1766](https://github.com/gcc-mirror/gcc/blob/releases/gcc-16.2.0/gcc/gcov.cc#L1764) explicitly removes artificial functions before output; absence from JSON is not nonexecution.\n\n'
               f'Manual audit: randomly sampled **20/178** original manual identities, seed `{MANUAL_SEED}`. **20 accepted, 0 wrong** (14 counterparts and 6 absent/inlined wrappers); no full-category re-review trigger. The marked table rows identify the exact sample. Source checks cover exact field-pass sorting, wildcard equality/prefix filtering, complete/incomplete keyspace and prefix-marker construction, indexed reading membership, token removal and mask predicates. Population/sample hashes guard the selection independently of subsequent reclassification.\n\n')
    table_header = ('| Kind | Pin source/call site | Full signature | USR or signature identity | Witness export | Witness path / resolution | Classification | Credited differential | Counterpart or cited reason / remaining work |\n'
                    '| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n')
    text = header + table_header + '\n'.join('| ' + ' | '.join(ledger.escape(c) for c in row) + ' |' for row in rows) + '\n'
    checks = ledger.mutation_check({key: '| ' + ' | '.join(ledger.escape(c) for c in row) + ' |'
                                   for key, row in functions.items()}, inventory)
    for result in checks: print(result, flush=True)
    if args.check:
        if args.output.read_text() != text: raise RuntimeError('refresh ledger differs')
    else:
        args.output.write_text(text)
    print(f'J refresh roots={len(state["roots"])} rows={len(functions)} baseline_executed={baseline_executed}/962 categories={dict(categories)} prior_unknown={dict(resolved)}', flush=True)
    print(f'Manual sample seed={MANUAL_SEED}; 20/178 accepted; population/sample hashes verified', flush=True)
    if categories['UNKNOWN-LIMIT'] or categories['NOT-EXECUTED-UNREVIEWED']:
        print('Remaining rows require their own coverage-gap issues; no closure claim', flush=True)
    if args.require_complete and (categories['UNKNOWN-LIMIT'] or categories['NOT-EXECUTED-UNREVIEWED']
                                 or state['sites'] or state['ambiguous'] or unmatched or incomplete):
        raise RuntimeError('exhaustive static closure remains partial; explicit dispatch/measurement limits persist')
