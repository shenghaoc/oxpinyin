"""Callgrind stops if the profiled and shipping libraries have different .text bytes.

To investigate a difference, disassemble both libraries with `objdump -d` and compare the output by hand.
"""
import hashlib
import re
import subprocess
import tempfile
from pathlib import Path

from ..runners import status


def compare(first, second) -> tuple[int, str]:
    """Compare the extracted .text bytes and return the status and report."""
    texts = []
    with tempfile.TemporaryDirectory(prefix='neutrality-') as scratch:
        for name, library in (('first', first), ('second', second)):
            destination = Path(scratch) / f'{name}.text'
            command = ['objcopy', '-O', 'binary', '--only-section=.text', str(library), str(destination)]
            try:
                done = subprocess.run(command, capture_output=True)
            except FileNotFoundError:
                return 127, ''
            if done.returncode:
                return status(done.returncode), ''
            text = destination.read_bytes()
            if not text:
                return 1, f'FAIL: {library} has an empty .text section\n'
            texts.append(text)

    profiled, shipping = texts
    report = (f'with debug info:    .text sha256 {hashlib.sha256(profiled).hexdigest()} ({len(profiled)} bytes)\n'
              f'without debug info: .text sha256 {hashlib.sha256(shipping).hexdigest()} ({len(shipping)} bytes)\n')
    if profiled == shipping:
        return 0, report + 'PASS: identical .text bytes\n'
    return 1, report + ('FAIL: .text bytes differ — neutrality unproven. '
                        'Diff the objdump -d output of the two libraries by hand.\n')


def macho_code(path):
    """Executable section addresses and bytes from a thin little-endian arm64 Mach-O."""
    import struct
    data = Path(path).read_bytes()
    if len(data) < 32 or struct.unpack_from('<II', data) != (0xfeedfacf, 0x100000c):
        raise ValueError('neutrality requires a thin arm64 Mach-O')
    count, size = struct.unpack_from('<II', data, 16)
    cursor, limit, sections = 32, 32 + size, {}
    if limit > len(data):
        raise ValueError('truncated Mach-O commands')
    for _ in range(count):
        if cursor + 8 > limit:
            raise ValueError('truncated Mach-O command')
        command, length = struct.unpack_from('<II', data, cursor)
        if length < 8 or cursor + length > limit:
            raise ValueError('invalid Mach-O command length')
        if command == 0x19:
            if length < 72:
                raise ValueError('truncated segment')
            nsects = struct.unpack_from('<I', data, cursor + 64)[0]
            if 72 + nsects * 80 > length:
                raise ValueError('truncated sections')
            for i in range(nsects):
                offset = cursor + 72 + i * 80
                name, segment, address, size, fileoff = struct.unpack_from('<16s16sQQI', data, offset)
                flags = struct.unpack_from('<I', data, offset + 64)[0]
                if flags & (0x80000000 | 0x400):
                    if fileoff + size > len(data):
                        raise ValueError('truncated executable section')
                    key = (segment.rstrip(b'\0'), name.rstrip(b'\0'))
                    if key in sections:
                        raise ValueError('duplicate executable section')
                    sections[key] = (address, data[fileoff:fileoff+size])
        cursor += length
    if not sections:
        raise ValueError('no executable sections')
    return sections


def compare_macho(symbols, plain):
    """Native timing requires identical code AND layout; no multiset allowance."""
    from . import bench
    if macho_code(symbols) != macho_code(plain):
        raise ValueError('Mach-O executable sections or addresses differ')
    listings = []
    for path in (symbols, plain):
        done = bench.capture(['xcrun', 'llvm-objdump', '--disassemble', '--no-symbolic-operands', str(path)])
        if done.status:
            raise ValueError('arm64 disassembly failed')
        # Keep addresses, bytes and operands. Ignore filenames and symbol labels only.
        rows = [line.strip() for line in done.stdout.decode().splitlines()
                if re.match(r'^\s*[0-9a-fA-F]+:', line)]
        if not rows:
            raise ValueError('empty arm64 disassembly')
        listings.append(rows)
    if listings[0] != listings[1]:
        raise ValueError('arm64 disassembly differs')
    return 'identical executable sections, addresses and arm64 disassembly'
