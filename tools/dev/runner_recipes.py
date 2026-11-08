"""The C++ smoke recipe, the one runner that compiles a driver."""
from collections.abc import Sequence
import re

from .runners import Context, status


def pinyin_needed(text: str) -> str:
    return '\n'.join(re.findall(r'Shared library: \[(libpinyin[^]]*)\]', text))


def pinyin_resolved(text: str) -> str:
    return '\n'.join(re.findall(r'libpinyin\.so\.15 => ([^ ]*)', text))


def cpp_smoke(ctx: Context, args: Sequence[str]) -> int:
    ctx.emit('--- building oxpinyin-capi for the C++ smoke gate ---')
    subject = ctx.artifact()
    work = ctx.scratch()
    user = work / 'user'
    user.mkdir()
    system = work / 'sys'
    ctx.services.mini_fixture(system)
    libraries = work / 'lib'
    libraries.mkdir()
    (libraries / 'libpinyin_capi.so').symlink_to(subject)
    (libraries / 'libpinyin.so.15').symlink_to('libpinyin_capi.so')
    ctx.emit('--- compiling C++ smoke TU against pinyin.h ---')
    import shlex
    flags = shlex.split(ctx.env['GLIB_CFLAGS']) if ctx.env.get('GLIB_CFLAGS') else ctx.flags('--cflags')
    driver = work / 'cpp-smoke'
    ctx.checked(['g++', '-std=c++17', '-Wall', '-Wextra', '-Werror', '-O2', *flags,
                 '-I' + str(ctx.settings.root / 'crates/oxpinyin-capi'), 'cpp-smoke.cc',
                 '-L' + str(libraries), '-Wl,-rpath,' + str(libraries), '-lpinyin_capi', '-o', driver])
    needed = pinyin_needed(ctx.checked(['readelf', '-d', driver], capture=True).stdout.decode())
    if needed != 'libpinyin.so.15':
        ctx.emit(f"fatal: smoke binary needs '{needed}', expected libpinyin.so.15")
        return 1
    resolved = pinyin_resolved(ctx.checked(['ldd', driver], environment=dict(ctx.env, LD_LIBRARY_PATH=''), capture=True).stdout.decode())
    if not resolved.startswith(str(libraries) + '/'):
        ctx.emit(f"fatal: libpinyin.so.15 resolved to '{resolved}', not the build under test")
        return 1
    ctx.emit('--- running C++ smoke TU ---')
    result = ctx.run(driver, driver, [system, user], side='subject')
    ctx.write(result.stdout)
    ctx.write(result.stderr, error=True)
    if result.returncode:
        return status(result.returncode)
    ctx.emit('cpp-smoke: ok')
    return 0


RECIPES = {'cpp-smoke': cpp_smoke}
