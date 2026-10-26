#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Build the pages.test.py object fixtures from pages.c, once, on macOS.

    uv run --no-project --managed-python python -X lazy_imports=normal \\
        tools/dev/measure/fixtures/build-pages.py [--elf-cc CLANG]

Writes, next to this script:

  pages-elf-x86_64.so        ELF64 x86_64 shared object, linked by ld.lld
  pages-macho-arm64.dylib    thin arm64 Mach-O dylib, linked by Xcode's ld
  pages-macho-x86_64.dylib   thin x86_64 Mach-O dylib, linked by Xcode's ld

and prints each file's size and SHA-256 for the record in pages.test.py.
The Mach-O dylibs need Xcode's clang (`xcrun clang`); the ELF object needs
a clang that can target x86_64-unknown-linux-gnu and an ld.lld beside it
(--elf-cc, default `clang` on PATH). -nostdlib keeps the ELF object free of
any C library, so no Linux sysroot is needed. Other toolchains give other
bytes, and the offsets pages.test.py expects would have to be re-derived
from llvm-objdump, otool and nm.
"""
import argparse
import hashlib
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
SOURCE = HERE / 'pages.c'
FLAGS = ['-Os', '-g0']


def build(elf_cc):
    """Compile and link the three fixtures; return their paths."""
    outputs = []
    for arch in ('arm64', 'x86_64'):
        output = HERE / f'pages-macho-{arch}.dylib'
        subprocess.run(['xcrun', 'clang', *FLAGS, '-arch', arch, '-mmacosx-version-min=11.0',
                        '-dynamiclib', '-install_name', '@rpath/pages.dylib',
                        '-o', output, SOURCE], check=True)
        outputs.append(output)
    output = HERE / 'pages-elf-x86_64.so'
    subprocess.run([elf_cc, '--target=x86_64-unknown-linux-gnu', *FLAGS, '-fPIC', '-nostdlib',
                    '-shared', '-fuse-ld=lld', '-o', output, SOURCE], check=True)
    outputs.append(output)
    # Fixtures are data: commit them without the linker's execute bits.
    for output in outputs:
        output.chmod(0o644)
    return outputs


def main(argv=None):
    """Build the fixtures and print their sizes and digests; return 1 off macOS."""
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--elf-cc', default='clang', help='clang with ld.lld for the ELF object')
    args = parser.parse_args(argv)
    if sys.platform != 'darwin':
        print('build-pages: needs macOS with Xcode for the Mach-O fixtures', file=sys.stderr)
        return 1
    for path in build(args.elf_cc):
        data = path.read_bytes()
        print(f'{path.name}\t{len(data)}\t{hashlib.sha256(data).hexdigest()}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
