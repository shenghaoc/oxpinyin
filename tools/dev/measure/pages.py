"""Map newly resident file pages to object sections and symbols (#1163).

The shared core of per-page residency attribution. The teardown campaign's
cold mode (tools/bisection/teardown.py) reports through it; the native
controller does not call it yet. Standard library only: objects are parsed
with struct, never readelf, nm or otool.

A residency vector is one file's mincore result (tools.dev.mman.vector): one
byte per host page, resident when bit 0 is set (Darwin sets other flag bits
too). newly_resident compares the vectors read at two checkpoints, and
checkpoint_pairs names the pairs of a sample's checkpoint sequence. Each newly
resident page of an object is reported with every segment, section and symbol
it overlaps; pages of data and user files are reported as byte ranges. A
campaign with several rounds counts the rounds that saw each page (tally) and
puts that count on each row (object_tally_rows, data_tally_rows).

Rows carry the object format's own field names. ELF rows have elf_sections and
elf_symbols; Mach-O rows have macho_segments, macho_sections and macho_symbols;
data rows have offset_range. A Mach-O name never appears in an ELF-named field.

Supported objects: ELF64 little-endian (sections from the section headers,
symbols from .symtab and .dynsym) and thin 64-bit little-endian Mach-O for
arm64 or x86_64 (LC_SEGMENT_64 segments and sections, LC_SYMTAB symbols).
Universal Mach-O selects the host architecture; unsupported formats raise ValueError. An ELF symbol spans
its st_size; zero-size, section and file symbols are left out. A Mach-O symbol
has no size, so it spans to the next symbol address in its section.

Limits (LIMITS, docs/runbooks/benches.md): a mincore difference includes
readahead, so a newly resident page is not necessarily a faulted page, and
pausing at a checkpoint can let outstanding readahead finish; a page the
process wrote is resident too. Parsing reads the whole object into the page
cache, so parse it after the sample's last residency read, or parse a
byte-identical copy.
"""
import bisect
import json
import os
import platform
from pathlib import Path
import struct

# <elf.h>
ELF_MAGIC = b'\x7fELF'
ELFCLASS64 = 2
ELFDATA2LSB = 1
PN_XNUM = 0xffff
PT_TLS = 7
SHN_UNDEF = 0
SHN_LORESERVE = 0xff00
SHN_XINDEX = 0xffff
SHT_SYMTAB = 2
SHT_NOBITS = 8
SHT_DYNSYM = 11
STT_SECTION = 3
STT_FILE = 4
STT_TLS = 6

# <mach-o/loader.h>, <mach-o/fat.h>, <mach-o/nlist.h> and <mach/machine.h>.
# Magic numbers are read little-endian; a fat header is big-endian on disk.
MH_MAGIC = 0xfeedface
MH_CIGAM = 0xcefaedfe
MH_MAGIC_64 = 0xfeedfacf
MH_CIGAM_64 = 0xcffaedfe
FAT_MAGICS = (b'\xca\xfe\xba\xbe', b'\xca\xfe\xba\xbf', b'\xbe\xba\xfe\xca', b'\xbf\xba\xfe\xca')
CPU_TYPES = {0x0100000c: 'arm64', 0x01000007: 'x86_64'}
LC_SYMTAB = 0x2
LC_SEGMENT_64 = 0x19
SECTION_TYPE = 0xff
ZEROFILL_TYPES = (0x1, 0xc, 0x12)  # S_ZEROFILL, S_GB_ZEROFILL, S_THREAD_LOCAL_ZEROFILL
N_STAB = 0xe0
N_TYPE = 0x0e
N_SECT = 0xe

# What a report of newly resident pages says about itself, for the reader of
# its output (docs/runbooks/benches.md, "Per-page residency attribution").
LIMITS = (
    'a mincore difference includes readahead, so a newly resident page is not necessarily a faulted page',
    'pausing at a checkpoint can let outstanding readahead finish, so a page requested before one checkpoint '
    'can show as newly resident at the next',
    'a page the process wrote is resident too, so a file that grew or was rewritten shows its written pages '
    'as newly resident',
)


def _check_page_size(page_size):
    """Raise ValueError unless PAGE_SIZE is a positive power of two."""
    if type(page_size) is not int or page_size <= 0 or page_size & (page_size - 1):
        raise ValueError(f'page size must be a positive power of two: {page_size!r}')


def newly_resident(before, after, page_size):
    """Return the byte offsets of the pages resident in AFTER but not in BEFORE.

    BEFORE and AFTER are one file's mincore vectors from two checkpoints,
    one value per page of PAGE_SIZE bytes; only bit 0 means resident. A page
    past the end of BEFORE was not in the file then, so it is newly resident
    when AFTER shows it resident.
    """
    _check_page_size(page_size)
    return [index * page_size for index, value in enumerate(after)
            if value & 1 and not (index < len(before) and before[index] & 1)]


def checkpoint_pairs(points):
    """Return the consecutive (earlier, later) pairs of an ordered checkpoint sequence.

    Only neighbours pair: M0, M1, M2, M3 give (M0, M1), (M1, M2) and (M2, M3),
    never (M0, M2), because a page that became resident at M1 and stayed so
    belongs to the first interval alone. Fewer than two checkpoints give no pair.
    """
    return list(zip(points, points[1:]))


def _unpack(layout, data, offset, what):
    """Unpack a struct LAYOUT at OFFSET, raising ValueError unless it lies inside DATA.

    The bounds are checked before unpacking: unpack_from cannot take an offset
    of 2**63 or more, which a corrupt 64-bit ELF header field can hold.
    """
    if offset < 0 or offset + struct.calcsize(layout) > len(data):
        raise ValueError(f'truncated {what} at offset {offset}')
    return struct.unpack_from(layout, data, offset)


def _string(data, start, end, what):
    """Return the NUL-terminated name at START in the string table ending at END."""
    end = min(end, len(data))
    stop = data.find(b'\0', start, end) if 0 <= start < end else -1
    if stop < 0:
        raise ValueError(f'{what} name at offset {start} is outside its string table')
    return data[start:stop].decode('utf-8', 'backslashreplace')


def _in_file(data, start, end, what):
    """Raise ValueError when the byte range [START, END) runs past the end of DATA."""
    if end > len(data):
        raise ValueError(f'{what} [{start:#x}, {end:#x}) runs past the end of the file')


def _elf(data):
    """Return the ELF64 little-endian file ranges of sections and sized symbols."""
    if len(data) < 6:
        raise ValueError('truncated ELF identification')
    if data[4] != ELFCLASS64:
        raise ValueError('unsupported ELF: only 64-bit objects are mapped')
    if data[5] != ELFDATA2LSB:
        raise ValueError('unsupported ELF: only little-endian objects are mapped')
    (_, _, _, _, _, phoff, shoff, _, _, phentsize, phnum, shentsize, shnum,
     shstrndx) = _unpack('<16sHHIQQQIHHHHHH', data, 0, 'ELF header')
    if (shoff and (not shnum or shstrndx == SHN_XINDEX)) or phnum == PN_XNUM:
        raise ValueError('unsupported ELF: extended section or program header numbering')
    if (shnum and shentsize != 64) or (phnum and phentsize != 56):
        raise ValueError('unsupported ELF: unexpected header entry size')
    # Linked objects give an STT_TLS symbol's st_value relative to the TLS
    # segment; relocatable objects have no PT_TLS and give it per section.
    tls = 0
    for index in range(phnum):
        kind, _, _, address, _, _, _, _ = _unpack('<IIQQQQQQ', data, phoff + 56 * index, 'program header')
        if kind == PT_TLS:
            tls = address
    headers = [_unpack('<IIQQQQIIQQ', data, shoff + 64 * index, 'section header') for index in range(shnum)]
    if headers and shstrndx >= shnum:
        raise ValueError(f'section name table index {shstrndx} is out of range')
    sections, symbols = set(), set()
    for index, (name, kind, _, _, offset, size, *_) in enumerate(headers):
        if index and kind != SHT_NOBITS and size:
            strtab = headers[shstrndx]
            label = _string(data, strtab[4] + name, strtab[4] + strtab[5], 'section')
            _in_file(data, offset, offset + size, 'section ' + label)
            sections.add((offset, offset + size, label))
    for kind, link, offset, size, entsize in ((h[1], h[6], h[4], h[5], h[9]) for h in headers):
        if kind not in (SHT_SYMTAB, SHT_DYNSYM):
            continue
        if entsize != 24 or link >= shnum:
            raise ValueError('unsupported ELF: malformed symbol table')
        strtab = headers[link]
        for entry in range(size // 24):
            name, info, _, shndx, value, extent = _unpack('<IBBHQQ', data, offset + 24 * entry, 'symbol')
            if shndx == SHN_XINDEX:
                raise ValueError('unsupported ELF: extended section indices')
            if (not extent or (info & 0xf) in (STT_SECTION, STT_FILE) or shndx == SHN_UNDEF or
                    shndx >= SHN_LORESERVE):
                continue
            if shndx >= shnum:
                raise ValueError(f'symbol section index {shndx} is out of range')
            _, section_kind, _, address, section_offset, section_size, *_ = headers[shndx]
            if section_kind == SHT_NOBITS:
                continue
            start = value + (tls if (info & 0xf) == STT_TLS else 0) - address + section_offset
            start, end = max(start, section_offset), min(start + extent, section_offset + section_size)
            label = _string(data, strtab[4] + name, strtab[4] + strtab[5], 'symbol')
            if start < end and label:
                symbols.add((start, end, label))
    return dict(elf_sections=sorted(sections), elf_symbols=sorted(symbols))


def _macho(data):
    """Return the thin Mach-O file ranges of segments, sections and symbols."""
    if data[:4] in FAT_MAGICS:
        raise ValueError('fat Mach-O is not supported: map one thin slice (lipo -thin)')
    magic = int.from_bytes(data[:4], 'little')
    if magic in (MH_MAGIC, MH_CIGAM):
        raise ValueError('unsupported Mach-O: only 64-bit objects are mapped')
    if magic == MH_CIGAM_64:
        raise ValueError('unsupported Mach-O: only little-endian objects are mapped')
    if magic != MH_MAGIC_64:
        raise ValueError('unknown object format: not ELF or Mach-O')
    _, cputype, _, _, ncmds, sizeofcmds, _, _ = _unpack('<IiiIIIII', data, 0, 'Mach-O header')
    if cputype not in CPU_TYPES:
        raise ValueError(f'unsupported Mach-O CPU type {cputype:#x}: only arm64 and x86_64 are mapped')
    segments, sections, table = set(), [], None
    offset, end = 32, 32 + sizeofcmds
    _in_file(data, offset, end, 'load commands')
    for _ in range(ncmds):
        command, size = _unpack('<II', data, offset, 'load command')
        if size < 8 or offset + size > end:
            raise ValueError(f'malformed load command at offset {offset}')
        if command == LC_SEGMENT_64:
            _, _, name, _, _, fileoff, filesize, _, _, count, _ = _unpack('<II16sQQQQiiII', data, offset, 'segment')
            if 72 + 80 * count > size:
                raise ValueError(f'malformed segment command at offset {offset}')
            label = name.split(b'\0', 1)[0].decode('utf-8', 'backslashreplace')
            if filesize:
                _in_file(data, fileoff, fileoff + filesize, 'segment ' + label)
                segments.add((fileoff, fileoff + filesize, label))
            for index in range(count):
                sectname, segname, address, length, start, _, _, _, flags, _, _, _ = _unpack(
                    '<16s16sQQIIIIIIII', data, offset + 72 + 80 * index, 'section')
                label = ','.join(part.split(b'\0', 1)[0].decode('utf-8', 'backslashreplace')
                                 for part in (segname, sectname))
                stored = bool(length) and (flags & SECTION_TYPE) not in ZEROFILL_TYPES
                if stored:
                    _in_file(data, start, start + length, 'section ' + label)
                sections.append((address, length, start, label, stored))
        elif command == LC_SYMTAB:
            if size < 24:
                raise ValueError(f'malformed symbol table command at offset {offset}')
            table = _unpack('<IIIIII', data, offset, 'symbol table command')[2:]
        offset += size
    # n_sect numbers sections from 1 across all segments in load command order.
    defined = {}
    symoff, nsyms, stroff, strsize = table or (0, 0, 0, 0)
    _in_file(data, symoff, symoff + nsyms * 16, 'symbol table')
    _in_file(data, stroff, stroff + strsize, 'string table')
    for entry in range(nsyms):
        name, kind, ordinal, _, value = _unpack('<IBBHQ', data, symoff + 16 * entry, 'symbol')
        if kind & N_STAB or (kind & N_TYPE) != N_SECT or not 0 < ordinal <= len(sections):
            continue
        label = _string(data, stroff + name, stroff + strsize, 'symbol')
        if label:
            defined.setdefault(ordinal, []).append((value, label))
    symbols = set()
    for ordinal, entries in defined.items():
        address, length, start, _, stored = sections[ordinal - 1]
        if not stored:
            continue
        values = sorted({value for value, _ in entries} | {address + length})
        for value, label in entries:
            later = bisect.bisect_right(values, value)
            stop = values[later] if later < len(values) else value
            first, last = max(value, address) - address + start, min(stop, address + length) - address + start
            if first < last:
                symbols.add((first, last, label))
    return dict(macho_segments=sorted(segments),
                macho_sections=sorted((start, start + length, label)
                                      for _, length, start, label, stored in sections if stored),
                macho_symbols=sorted(symbols))


def _host_slice(data):
    """Select a universal Mach-O slice and return bytes, offset and architecture."""
    magic = data[:4]
    endian = '>' if magic in (b'\xca\xfe\xba\xbe', b'\xca\xfe\xba\xbf') else '<'
    wide = magic in (b'\xca\xfe\xba\xbf', b'\xbf\xba\xfe\xca')
    _, count = _unpack(endian + 'II', data, 0, 'fat header')
    architecture = platform.machine().lower()
    architecture = {'aarch64': 'arm64', 'amd64': 'x86_64'}.get(architecture, architecture)
    layout = endian + ('IIQQII' if wide else 'IIIII')
    size = struct.calcsize(layout)
    for index in range(count):
        fields = _unpack(layout, data, 8 + index * size, 'fat architecture')
        cpu, _, offset, length = fields[:4]
        if CPU_TYPES.get(cpu) == architecture:
            _in_file(data, offset, offset + length, 'fat slice')
            if length < 32:
                raise ValueError('truncated fat slice')
            slice_data = data[offset:offset + length]
            if _unpack('<I', slice_data, 4, 'slice CPU type')[0] != cpu:
                raise ValueError('fat slice CPU type does not match its header')
            return slice_data, offset, architecture
    raise ValueError(f'fat Mach-O has no slice for host architecture {architecture}')


def object_layout(path, *, data=None):
    """Read an object and return its named file ranges, keyed by output field.

    Each value is a sorted list of [start, end) file byte ranges with names:
    sections and symbols for ELF; segments, sections and symbols for Mach-O.
    DATA supplies bytes already read from a verified descriptor; when given,
    PATH is only a diagnostic name and is not opened. Raise ValueError,
    naming PATH, for an unsupported or malformed file.
    """
    if data is None:
        data = Path(path).read_bytes()
    try:
        if data[:4] == ELF_MAGIC:
            return _elf(data)
        if data[:4] in FAT_MAGICS:
            slice_data, offset, _ = _host_slice(data)
            return {field: [(start + offset, end + offset, name) for start, end, name in ranges]
                    for field, ranges in _macho(slice_data).items()}
        return _macho(data)
    except ValueError as error:
        raise ValueError(f'{path}: {error}') from None


def object_rows(layout, offsets, page_size, name):
    """Return one row per page offset of an object, listing every overlapping name.

    LAYOUT comes from object_layout. A page that spans several segments,
    sections or symbols lists all of them, in file order and without
    duplicate names.
    """
    _check_page_size(page_size)
    rows = [dict(file=name, page_offset=offset) for offset in offsets]
    for field, ranges in layout.items():
        found = {offset // page_size: {} for offset in offsets}
        for start, end, label in ranges:
            for page in range(start // page_size, (end - 1) // page_size + 1):
                if page in found:
                    found[page][label] = None
        for row in rows:
            row[field] = list(found[row['page_offset'] // page_size])
    return rows


def data_rows(offsets, page_size, size, name):
    """Return the newly resident byte ranges of a data or user file.

    Join adjacent page offsets into half-open offset_range [start, end)
    rows and clip the last page to the file SIZE.
    """
    _check_page_size(page_size)
    rows = []
    for offset in sorted(offsets):
        if offset >= size:
            continue
        end = min(offset + page_size, size)
        if rows and rows[-1]['offset_range'][1] == offset:
            rows[-1]['offset_range'][1] = end
        else:
            rows.append(dict(file=name, offset_range=[offset, end]))
    return rows


def tally(rounds):
    """Count, for each page offset, the rounds in which it was newly resident.

    ROUNDS holds one newly_resident result per round. Readahead timing can
    differ between rounds, so a campaign reports how often each page was seen
    rather than one round's set.
    """
    counts = {}
    for offsets in rounds:
        for offset in set(offsets):
            counts[offset] = counts.get(offset, 0) + 1
    return counts


def object_tally_rows(layout, counts, page_size, name):
    """Return object_rows for the offsets in COUNTS, each row with in_rounds.

    in_rounds is the number of rounds that saw the row's page newly resident.
    """
    rows = object_rows(layout, sorted(counts), page_size, name)
    for row in rows:
        row['in_rounds'] = counts[row['page_offset']]
    return rows


def data_tally_rows(counts, page_size, size, name):
    """Return data_rows for the offsets in COUNTS, each row with in_rounds.

    Pages join one offset_range only when their round counts are equal, so a
    row's in_rounds is true of every page in it. Rows are in file order.
    """
    rows = []
    for count in sorted(set(counts.values())):
        runs = data_rows([offset for offset, seen in counts.items() if seen == count], page_size, size, name)
        rows.extend(dict(row, in_rounds=count) for row in runs)
    return sorted(rows, key=lambda row: row['offset_range'])


def object_report(path, before, after, page_size, name=None):
    """Map an object's pages newly resident between two vectors to rows."""
    offsets = newly_resident(before, after, page_size)
    return object_rows(object_layout(path), offsets, page_size, str(path) if name is None else name)


def data_report(path, before, after, page_size, name=None):
    """Report a data or user file's newly resident pages as byte ranges.

    The file's current size, read without touching its pages, clips the
    last range.
    """
    offsets = newly_resident(before, after, page_size)
    return data_rows(offsets, page_size, os.stat(path).st_size, str(path) if name is None else name)


def text(rows):
    """Render rows as text: one sorted, ASCII-escaped JSON object per line."""
    return ''.join(json.dumps(row, sort_keys=True, ensure_ascii=True) + '\n' for row in rows)
