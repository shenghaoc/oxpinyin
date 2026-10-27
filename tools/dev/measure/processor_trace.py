"""Compare an xctrace Processor Trace of one startup with the Mach-O order file (#1171).

  python -m tools.dev measure processor-trace --trace init.trace \\
      --library "$PREFIX/lib/libpinyin.15.0.0.dylib" \\
      --policy tools/packaging/function-order/bdb/libpinyin.order

The Processor Trace instrument (Xcode 16.3 or newer on an Apple M4 or newer
Mac running macOS 15.4 or newer) reconstructs every function call of the
traced process from the CPU's branch trace. Its intervals table has one row
per call: the function's binary and Mach-O vm offset, and the call's own
instructions and cycles. This recipe exports that table with xctrace, or
reads an export named by --intervals, maps the library's offsets to its
__TEXT,__text symbols with llvm-nm, resolves the ELF policy against the same
symbol table through tools.packaging.function_order (the identities the
order file was derived with), and prints one JSON record: the functions the
trace executed in the library, which of them the order file names by policy
tier, the executed functions the policy does not name, could not resolve, or
names under another instantiating crate, and the ordered init-tier functions
the trace did not execute. The export
goes to a temporary directory unless --export-dir names one; nothing else is
kept. It changes no policy: what it suggests is a proposal.
"""
import argparse
import bisect
from collections import defaultdict
from collections.abc import Mapping, Sequence
import json
import os
from pathlib import Path
import re
import tempfile
from xml.etree import ElementTree

from .. import pool, runners, slot
from tools.packaging import function_order

TABLE = 'processor-trace-intervals'
XPATH = f'/trace-toc/run[@number="1"]/data/table[@schema="{TABLE}"]'


def _value(values, element):
    """The element's own value, or the earlier value it refers to."""
    return values[element.attrib['id'] if 'id' in element.attrib else element.attrib['ref']]


def intervals(path):
    """Calls, instructions and cycles per (binary, vm offset) in an exported intervals table.

    xctrace writes each value once, with an id, and refers back to it by ref.
    A text-location names its binary after two spaces in its fmt attribute
    and carries the call's mach-o-vm-offset; the two uint64 columns are the
    call's own instructions and cycles.
    """
    values = {}
    totals = defaultdict(lambda: [0, 0, 0])
    rows = 0
    node = None
    with open(path, 'rb') as stream:
        for event, element in ElementTree.iterparse(stream, events=('start', 'end')):
            rows, node = _interval_event(event, element, values, totals, rows, node)
    if not rows:
        raise ValueError(f'no {TABLE} rows in {path}')
    return dict(totals)


def _interval_event(event, element, values, totals, rows, node):
    """Fold one iterparse event into the tables; return the row count and the node element."""
    tag = element.tag
    if event == 'start':
        return rows, element if tag == 'node' else node
    if tag in ('mach-o-vm-offset', 'uint64'):
        if 'id' in element.attrib:
            values[element.attrib['id']] = int(element.text)
    elif tag == 'text-location':
        if 'id' in element.attrib:
            fmt = element.attrib.get('fmt', '')
            binary = fmt.rsplit('  ', 1)[1] if '  ' in fmt else ''
            offset = next((_value(values, child) for child in element if child.tag == 'mach-o-vm-offset'), None)
            values[element.attrib['id']] = (binary, offset)
    elif tag == 'row':
        rows += 1
        location = None
        counters = []
        for child in element:
            if child.tag == 'text-location':
                location = _value(values, child)
            elif child.tag == 'uint64':
                counters.append(_value(values, child))
        if location is not None and location[1] is not None:
            entry = totals[location]
            entry[0] += 1
            if len(counters) >= 2:
                entry[1] += counters[0]
                entry[2] += counters[1]
        element.clear()
        if node is not None and rows % 65536 == 0:
            node.clear()
    return rows, node


def text_symbols(darwin):
    """(address, raw name) of every __TEXT,__text symbol in llvm-nm's darwin format, by address."""
    return sorted((address, raw) for raw, address, section in function_order.macho_records(darwin)
                  if section == function_order.MACHO_TEXT)


def attribute(totals, symbols, library):
    """Calls, instructions and cycles per executed function of LIBRARY, by symbol extent.

    A Mach-O symbol spans to the next symbol; an offset before the first one
    is counted as unmapped. Return (per raw name, unmapped offsets).
    """
    addresses = [address for address, _ in symbols]
    executed = defaultdict(lambda: [0, 0, 0])
    unmapped = 0
    for (binary, offset), (calls, instructions, cycles) in totals.items():
        if binary != library:
            continue
        index = bisect.bisect_right(addresses, offset) - 1
        if index < 0:
            unmapped += 1
            continue
        entry = executed[symbols[index][1]]
        entry[0] += calls
        entry[1] += instructions
        entry[2] += cycles
    return dict(executed), unmapped


def policy_rows(policy):
    """(soname, [(tier, name)]) of an ELF policy file; the AWK reader validates it."""
    lines = Path(policy).read_text(encoding='utf-8').splitlines()
    header = lines[0].split('\t') if lines else []
    if len(header) != 2:
        raise ValueError(f'policy header does not name a SONAME: {policy}')
    done = function_order.resolve('policy', policy=policy, soname=header[1])
    if done.status:
        raise ValueError(f'awk: {done.stderr.decode(errors="replace").strip()}')
    names = done.stdout.decode().splitlines()
    rows = [tuple(line.split('\t')) for line in lines[1:]]
    if [name for _, name in rows] != names:
        raise ValueError(f'policy rows do not match the validated names: {policy}')
    return header[1], rows


def compare(executed, rows, symbols):
    """The report of EXECUTED (per raw name) against the policy ROWS resolved on SYMBOLS."""
    names = [name for _, name in rows]
    tiers = dict((name, tier) for tier, name in rows)
    ordered, unresolved = function_order.macho_order(names, symbols)
    identity = {raw: name for name, raw, _, section in symbols
                if section == function_order.MACHO_TEXT and name is not None}
    tier_of = {raw: tiers[identity[raw]] for raw in ordered}
    present = set(executed)

    def row(raw):
        calls, instructions, cycles = executed[raw]
        return dict(function=identity.get(raw, raw), symbol=raw, calls=calls,
                    instructions=instructions, cycles=cycles)

    executed_ordered = [raw for raw in ordered if raw in present]
    not_ordered = sorted((raw for raw in present if raw not in tier_of), key=lambda raw: -executed[raw][1])
    unresolved_set = set(unresolved)
    # A policy path whose instantiating crate differs names the same generic
    # body that this target's LTO kept from another crate.
    crateless = {re.sub(r' \[instantiating crate: [^\]]+\]$', '', name): name for name in unresolved}
    instructions = sum(entry[1] for entry in executed.values())
    inside = sum(executed[raw][1] for raw in executed_ordered)

    def policy(raw):
        name = identity.get(raw)
        if name in unresolved_set:
            return 'unresolved'
        if name is not None and re.sub(r' \[instantiating crate: [^\]]+\]$', '', name) in crateless:
            return 'other-crate'
        return 'absent'
    return dict(
        executed=dict(functions=len(executed), calls=sum(entry[0] for entry in executed.values()),
                      instructions=instructions, cycles=sum(entry[2] for entry in executed.values())),
        ordered=dict(functions=len(ordered), init=sum(tier == 'init' for tier in tier_of.values()),
                     steady=sum(tier == 'steady' for tier in tier_of.values()), unresolved=unresolved),
        executed_ordered=dict(functions=len(executed_ordered),
                              init=sum(tier_of[raw] == 'init' for raw in executed_ordered),
                              steady=sum(tier_of[raw] == 'steady' for raw in executed_ordered),
                              instructions=inside),
        executed_not_ordered=[dict(row(raw), policy=policy(raw)) for raw in not_ordered],
        init_not_executed=[identity[raw] for raw in ordered if tier_of[raw] == 'init' and raw not in present],
        steady_executed=[identity[raw] for raw in executed_ordered if tier_of[raw] == 'steady'])


def export(trace, directory, env):
    """Export the intervals table of TRACE into DIRECTORY with xctrace; return the XML path."""
    output = Path(directory) / (TABLE + '.xml')
    output.parent.mkdir(parents=True, exist_ok=True)
    context = pool.UnitContext('processor-trace', slot.CANCELLED, environment=dict(env))
    done = context.launch(['xcrun', 'xctrace', 'export', '--input', str(trace), '--xpath', XPATH,
                           '--output', str(output)])
    if done.status:
        raise ValueError(f'xctrace export failed ({done.status}): {done.stderr.decode(errors="replace").strip()}')
    return output


def processor_trace(argv: Sequence[str], env: Mapping[str, str]) -> int:
    parser = argparse.ArgumentParser(prog='python -m tools.dev measure processor-trace',
                                     description='compare a Processor Trace of one startup with the Mach-O order file')
    parser.add_argument('--trace', type=Path, help='an xctrace Processor Trace recording, exported here')
    parser.add_argument('--intervals', type=Path, help=f'an exported {TABLE} table, instead of --trace')
    parser.add_argument('--library', type=Path, required=True, help='the traced unstripped dylib')
    parser.add_argument('--policy', type=Path, required=True, help='the ELF policy the order file was derived from')
    parser.add_argument('--export-dir', type=Path, help='keep the export in this directory (default: temporary)')
    args = parser.parse_args(argv)
    if bool(args.trace) == bool(args.intervals):
        parser.error('name exactly one of --trace and --intervals')
    for path in (args.trace or args.intervals, args.library, args.policy):
        if not path.exists():
            runners.emit(f'processor-trace: missing input {path}', error=True)
            return 1
    _, rows = policy_rows(args.policy)
    darwin, demangled = function_order.macho_tables(args.library)
    symbols = function_order.macho_symbols(darwin, demangled)
    with tempfile.TemporaryDirectory(prefix='processor-trace-') as scratch:
        if args.intervals:
            table = args.intervals
        else:
            table = export(args.trace, args.export_dir or scratch, env)
        totals = intervals(table)
    executed, unmapped = attribute(totals, text_symbols(darwin), args.library.name)
    if not executed:
        raise ValueError(f'the trace has no call in {args.library.name}')
    report = compare(executed, rows, symbols)
    report['library'] = args.library.name
    report['unmapped_offsets'] = unmapped
    report['binaries'] = len({binary for binary, _ in totals})
    print(json.dumps({'processor-trace': report}, sort_keys=True), flush=True)
    return 0


RECIPES = {'processor-trace': processor_trace}
