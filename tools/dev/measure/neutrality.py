"""Is a debug-info build the same code as one without? (#1290)

The profiled artifact is built with debug info and the timed one without
(CARGO_PROFILE_RELEASE_DEBUG feeds cargo's crate metadata hash, hence every
mangled symbol name, hence link order), so the two .text sections hold the same
functions at different addresses. `compare` classifies a pair. Three outcomes,
and only the last is fatal:

  (a) identical .text bytes                  - fully neutral
  (b) same instruction multiset at different addresses
      - Ir totals are unaffected (the same instructions execute); the SIMULATED
        cache and branch figures a callgrind --cache-sim run prints are
        layout-sensitive and must be reported as qualitative context only,
        never as measurements
  (c) a different instruction multiset       - different code. Stop.

The multiset comparison keeps the FULL instruction text, not just the mnemonic:
same mnemonics with different registers or immediates is a real codegen
difference and must land in case (c). Only two things are normalized away, both
pure functions of where the linker placed code: the target of direct control
transfers (branch, call, loop) and rip-relative displacements (including their
sign). Anything else is compared verbatim.

The relocation count pairs text symbols by a stable identity with the
crate-disambiguator hash stripped from v0-mangled names (`Cs..._`), so a renamed
symbol pairs with itself instead of being miscounted. A handful of symbols still
rename structurally under fat LTO when the hash changes; those are paired by
identical normalized instruction bodies instead, and every tier of the
accounting is printed so nothing is silently guessed.

This is the former debuginfo-neutrality-lib.sh, whose report it reproduces. Its
text handling is the C locale's (byte order, no collation), as the images that
ran it had. Two behaviours of the shell stay as they were, and are named where
they matter: `#` ends an instruction on aarch64 as it ends a comment on x86-64,
and a tool that fails ends the comparison silently with its status. Standard
library only.
"""
import collections
import hashlib
import io
import re
import subprocess
import tempfile
from collections.abc import Callable, Iterator, Sequence
from pathlib import Path

from ..runners import emit, write

SUBSEP = '\x1c'
# POSIX [[:space:]] in the C locale. Python's \s also matches the Latin-1 bytes
# 0x85 and 0xa0 and the separators 0x1c-0x1f, which the listings are decoded to.
BLANK = '[ \t\n\v\f\r]'
COMMENT = re.compile(BLANK + '#.*$')
RIP_RELATIVE = re.compile(r'-?0x[0-9a-f]+\(%rip\)')
FUNCTION_HEADER = re.compile('^' + BLANK + r'*[0-9a-f]+ <.*>:$')
HEADER_PREFIX = re.compile('^' + BLANK + r'*[0-9a-f]+ <')
HEADER_SUFFIX = re.compile('>:$')
INSTRUCTION_ADDRESS = re.compile('^' + BLANK + r'*[0-9a-f]+:' + BLANK + '*')
WHITESPACE = re.compile(BLANK + '+')
ARM_BRANCH = re.compile(r'b(\.[a-z]+)?')
CRATE_HASH = re.compile('Cs[0-9A-Za-z]+_')
SHOWN_DIFFERENCES = 20

Run = Callable[..., subprocess.CompletedProcess]


class Stopped(Exception):
    """A tool failed where the shell function would have exited with its status."""

    def __init__(self, status: int):
        super().__init__(status)
        self.status = status


def lines(data: bytes) -> Iterator[str]:
    """The records of a tool's output, split on newlines only.

    Bytes decode one to one (Latin-1), so hashing and ordering see exactly the
    bytes the tool wrote.
    """
    for line in io.StringIO(data.decode('latin-1'), newline='\n'):
        yield line[:-1] if line.endswith('\n') else line


def instruction_rows(objdump: bytes) -> Iterator[tuple[str, str]]:
    """The (function, normalized instruction) pairs of an `objdump -d --no-show-raw-insn` listing.

    A header line `ADDRESS <name>:` names the function that follows; its
    instructions lose their address and take the three normalizations, in this
    order: a comment (whitespace, `#`, rest of line) is dropped and rip-relative
    operands become RIPREL; a direct branch or call keeps only its mnemonic and
    TARGET (an indirect one, `jmp *%rax`, stays whole); an adrp or adr keeps its
    register and PCREL. Every other instruction has the crate hash of any v0
    symbol it mentions replaced by CsH_. Instructions before the first header
    belong to the function ''.
    """
    current = ''
    for line in lines(objdump):
        if '#' in line:
            line = COMMENT.sub('', line, count=1)
        if '(%rip)' in line:
            line = RIP_RELATIVE.sub('RIPREL', line)
        if line.endswith('>:') and FUNCTION_HEADER.match(line):
            current = HEADER_SUFFIX.sub('', HEADER_PREFIX.sub('', line, count=1), count=1)
            continue
        if not line or 'file format' in line or line.startswith('Disassembly'):
            continue
        text = INSTRUCTION_ADDRESS.sub('', line, count=1)
        words = WHITESPACE.split(text)
        mnemonic, operand = words[0], words[1] if len(words) > 1 else ''
        if (mnemonic.startswith(('j', 'loop')) or mnemonic == 'call') and not operand.startswith('*'):
            text = mnemonic + ' TARGET'
        elif mnemonic == 'bl' or ARM_BRANCH.fullmatch(mnemonic):
            text = mnemonic + ' TARGET'
        elif mnemonic in ('cbz', 'cbnz', 'tbz', 'tbnz'):
            text = f'{mnemonic} {operand} TARGET'
        elif mnemonic in ('adrp', 'adr'):
            text = f'{mnemonic} {operand} PCREL'
        else:
            text = CRATE_HASH.sub('CsH_', text)
        yield current, text


def histogram(census: collections.Counter) -> list[str]:
    """The instruction forms by count, as `sort | uniq -c | sort -rn` prints them in the C locale.

    Counts descend and equal counts fall in descending byte order of the form.
    The count is right-aligned in seven columns, then a space.
    """
    ordered = sorted(census.items(), key=lambda item: (item[1], item[0]), reverse=True)
    return [f'{count:7d} {form}' for form, count in ordered]


def text_symbols(nm: bytes) -> list[str]:
    """The sorted, unique `NAME ADDRESS` rows of the text symbols in `nm --defined-only` output.

    A text symbol has type t or T. The crate hash of a v0 name is replaced by
    CsH_, which is what makes a renamed symbol pair with itself.
    """
    rows = set()
    for line in lines(nm):
        fields = re.findall('[^ \t\n]+', line)
        if len(fields) > 1 and ('t' in fields[1] or 'T' in fields[1]):
            name = fields[2] if len(fields) > 2 else ''
            rows.add(CRATE_HASH.sub('CsH_', f'{name} {fields[0]}'))
    return sorted(rows)


def addresses(rows: Sequence[str]) -> dict[str, list[str]]:
    """The addresses of each symbol name in sorted `NAME ADDRESS` rows."""
    result = {}
    for row in rows:
        name, _, address = row.partition(' ')
        result.setdefault(name, []).append(address)
    return result


def unpaired(first: dict[str, list[str]], second: dict[str, list[str]]) -> list[str]:
    """The names of FIRST that SECOND cannot pair one to one, as `comm -23` lists them.

    A name that occurs more often in FIRST than in SECOND is left over once for
    each extra occurrence. The empty name has nothing to count and is left out.
    """
    return sorted(name for name, found in first.items() if name
                  for _ in range(len(found) - len(second.get(name, ()))))


def pairing(first: dict[str, list[str]], second: dict[str, list[str]]) -> tuple[int, int]:
    """How many pairs `join` makes on the name, and how many of them differ in address.

    The pairing is a join: a name that occurs in both sides pairs every
    occurrence in FIRST with every occurrence in SECOND. The addresses of a name
    are distinct within a side, so the pairs that agree are the addresses the
    two sides share.
    """
    paired = moved = 0
    for name, found in first.items():
        other = second.get(name)
        if name and other:
            paired += len(found) * len(other)
            moved += len(found) * len(other) - len(set(found) & set(other))
    return paired, moved


def body_digests(objdump: bytes) -> list[tuple[str, str]]:
    """A (crate-hash-stripped name, digest) pair for each function with instructions.

    The digest covers the first tab-delimited chunk of each normalized
    instruction, each followed by a separator byte, in listing order across every
    block the function name heads. On aarch64 objdump separates a mnemonic from its
    operands with a tab, so there the digest covers the mnemonics only.
    """
    hashers = {}
    for name, text in instruction_rows(objdump):
        if name:
            hashers.setdefault(name, hashlib.sha256()).update((text.split('\t')[0] + SUBSEP).encode('latin-1'))
    return [(CRATE_HASH.sub('CsH_', name), hasher.hexdigest()) for name, hasher in hashers.items()]


def leftover_note(first_only: list[str], second_only: list[str], first_digests, second_digests) -> str:
    """How the symbols that did not pair by name pair by instruction body.

    Only a leftover count that is the same on both sides can be matched: the
    digests of the leftover names are compared as multisets.
    """
    if not first_only:
        return 'none'
    if len(first_only) != len(second_only):
        return f'asymmetric leftovers ({len(first_only)} vs {len(second_only)}) — not pairable'
    wanted = set(first_only)
    first_found = collections.Counter(digest for name, digest in first_digests() if name in wanted)
    wanted = set(second_only)
    second_found = collections.Counter(digest for name, digest in second_digests() if name in wanted)
    matched = sum((first_found & second_found).values())
    if matched == len(first_only):
        return f'{len(first_only)} paired by identical instruction body'
    return f'only {matched} of {len(first_only)} paired by instruction body'


def tool(run: Run, command: Sequence, *, quiet: bool) -> bytes:
    """Run a GNU tool and return its output; a failure ends the comparison with its status.

    QUIET tools had their stderr discarded and fail silently; the others show
    it, and a missing one is named.
    """
    command = [str(part) for part in command]
    try:
        done = run(command, capture_output=True)
    except FileNotFoundError:
        if not quiet:
            emit(f'{command[0]}: command not found', error=True)
        raise Stopped(127) from None
    if not quiet and done.stderr:
        write(done.stderr, error=True)
    if done.returncode:
        raise Stopped(128 - done.returncode if done.returncode < 0 else done.returncode)
    return done.stdout


def first_lines(text: bytes, count: int) -> list[str]:
    """The first COUNT lines of a tool's output, as `head` prints them."""
    rows = text.decode('utf-8', 'replace').split('\n')
    if rows[-1] == '':
        rows.pop()
    return rows[:count]


def compare(first, second, *, run: Run = subprocess.run) -> tuple[int, str]:
    """Classify FIRST (built with debug info) against SECOND; return (status, report).

    The report is what the shell function wrote to standard output. Status 0 is
    case (a) or (b); status 1 is case (c). A tool that fails (objcopy, objdump,
    nm; the first two silently) ends the comparison with its own status and the
    report so far, as it ended the shell function. Case (c) also ended it there:
    `diff` exits 1 on any difference, and under `set -e` with `pipefail` that
    stopped the function before the two closing lines ("Stop and report. ..."), so
    they never appeared and the report does not carry them.

    RUN is subprocess.run, or a stand-in with its signature, for the GNU tools.
    """
    report = []
    try:
        with tempfile.TemporaryDirectory(prefix='neutrality-') as scratch:
            status = classify(Path(scratch), str(first), str(second), run, report.append)
    except Stopped as stop:
        status = stop.status
    return status, ''.join(line + '\n' for line in report)


def classify(scratch: Path, first: str, second: str, run: Run, say: Callable[[str], object]) -> int:
    """The body of `compare`: say each report line and return the status.

    The tools run one after another, first object then second. Each is a few seconds of
    binutils inside a step of the caller's run (the differential's stage 2, or the manual
    probe), which owns any admission; a pool here would take the heavy slot to save that.
    """
    texts = []
    for name, path in (('first', first), ('second', second)):
        destination = scratch / f'{name}.text'
        tool(run, ['objcopy', '-O', 'binary', '--only-section=.text', path, destination], quiet=True)
        texts.append(destination.read_bytes())
    digests = [hashlib.sha256(text).hexdigest() for text in texts]
    say(f'with debug info:    .text sha256 {digests[0]} ({len(texts[0])} bytes)')
    say(f'without debug info: .text sha256 {digests[1]} ({len(texts[1])} bytes)')
    if digests[0] == digests[1]:
        say('PASS (a): identical .text — debug info changed nothing else')
        return 0

    # Full instruction-text multiset: order-independent, so pure reordering
    # cancels and any real selection change (register, immediate, mnemonic)
    # survives into case (c).
    listings = [tool(run, ['objdump', '-d', '--no-show-raw-insn', path], quiet=True) for path in (first, second)]
    censuses = [collections.Counter(text for _, text in instruction_rows(listing)) for listing in listings]
    forms = [histogram(census) for census in censuses]
    totals = [str(sum(census.values())) if census else '' for census in censuses]
    say(f'instructions (normalized full text): {totals[0]} vs {totals[1]}, {len(forms[0])} distinct forms')
    if forms[0] != forms[1]:
        say('FAIL (c): instruction multiset differs — this is different code.')
        files = []
        for name, rows in (('first', forms[0]), ('second', forms[1])):
            files.append(scratch / f'{name}.hist')
            files[-1].write_bytes(''.join(row + '\n' for row in rows).encode('latin-1'))
        try:
            done = run(['diff', *map(str, files)], capture_output=True)
        except FileNotFoundError:
            emit('diff: command not found', error=True)
            raise Stopped(127) from None
        if done.stderr:
            write(done.stderr, error=True)
        for row in first_lines(done.stdout, SHOWN_DIFFERENCES):
            say(row)
        return 1

    # Symbol accounting in two pairing tiers, printed in full. Tier 1 pairs by
    # name with the v0 crate-disambiguator hash stripped (`Cs...` becomes CsH_);
    # tier 2 pairs any leftovers by identical normalized instruction body.
    symbols = [addresses(text_symbols(tool(run, ['nm', '--defined-only', path], quiet=False)))
               for path in (first, second)]
    first_only, second_only = unpaired(symbols[0], symbols[1]), unpaired(symbols[1], symbols[0])
    paired, moved = pairing(symbols[0], symbols[1])
    note = leftover_note(first_only, second_only, lambda: body_digests(listings[0]),
                         lambda: body_digests(listings[1]))
    say('PASS (b): identical instruction multiset (full normalized text).')
    say(f'  Symbol accounting: {paired} symbols pair by hash-stripped name,')
    say(f'  {moved} of those at different addresses; unpaired after hash')
    say(f'  strip: {len(first_only)} — {note}.')
    say('  Ir totals are unaffected by relocation. The SIMULATED cache and')
    say('  branch figures are layout-sensitive and must be read with that')
    say('  caveat stated; never present them as measurements.')
    return 0
