"""Environment probes for the perf and profile tooling (#1290).

Both are manual diagnostics, run before a long job, never part of one:

  python -m tools.dev measure probe-debuginfo-neutrality [BACKEND-FEATURE]
  python -m tools.dev measure probe-egress

probe-debuginfo-neutrality: is a debug-info build the same code as the shipped
one? Profilers need symbols and the release profile ships without them, so any
profile-vs-timing comparison rests on an assumption, that adding debug info
changed only debug info, which is both testable and, on this workspace, false in
a way worth knowing. The default backend is Berkeley DB (since 2026-09-20; tkrzw
2026-09-05 to then), which needs libdb-dev; pass a peer feature to probe another
backend (tkrzw on a host with only libtkrzw, say). PKG in the environment names
the crate (default oxpinyin-capi).

The comparison itself is neutrality.compare, shared with the differential
driver's stage 2. Two things this exists to stop you getting wrong:

1. RUSTFLAGS="-Cdebuginfo=1" IS SILENTLY USELESS HERE. Cargo passes
   -Cstrip=debuginfo whenever the profile itself asks for no debug info, so the
   flag emits DWARF and cargo strips it straight back out. Measured on rustc
   1.97.1: the RUSTFLAGS-only artifact carries zero .debug_* sections and is
   byte-identical to the stock one. Use RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"
   to override Cargo's stripping without changing the profile or metadata hash.

2. ENABLING IT CAN CHANGE CODEGEN. The earlier measurement on
   2026-09-08 UTC, oxpinyin-capi with and without debug info has the same
   instruction multiset but different .text bytes: the profile's debug level
   feeds cargo's crate metadata hash, hence every mangled symbol name, hence link
   order. That observation is not a guarantee: #1337 reproduced different
   reserve_rehash specialization under fat LTO. The campaign now uses
   both RUSTFLAGS overrides and callgrind-control to compare against shipping;
   this probe keeps the independent profiles to diagnose their interaction.

So the probe classifies rather than passing or failing on a hash:

  (a) identical .text bytes            -> fully neutral
  (b) identical instruction multiset   -> Ir safe, simulated cache and branch
      at different addresses              figures layout-shifted
  (c) different instruction multiset   -> different code. Stop.

probe-egress: can this host reach what the perf and oracle images need? The
perf-matrix and callgrind images are not self-contained: they fetch a base image,
Debian packages, rustup, crates and the pinned libpinyin tree at build time. On a
network with an egress allowlist any one of those can be denied, and the failure
surfaces deep inside a long `docker build` as an opaque 403. Run this first. It
exits 0 if every host needed for a perf image build is reachable.

Measured on the Claude Code remote environment, 2026-09-08 UTC: the Debian
repositories and Docker Hub's blob CDN were denied, which blocks libtkrzw-dev and
so blocks the libpinyin cell entirely. github.com, static.rust-lang.org, the
crates registry and mirror.gcr.io were reachable. See the handoff on
perf/steady-cycle-constant-factor-mkeg4k.

A CONNECT denial hides its body from curl, so where an agent proxy exposes a
status endpoint this also prints the reason it recorded.
"""
import hashlib
import json
from pathlib import Path
import re
import shutil
import tempfile
from collections.abc import Mapping, Sequence

from ..runners import Finished, emit, write
from . import bench, neutrality

ROOT = bench.ROOT
BUILDS = (('stock', {'CARGO_TERM_QUIET': 'true'}),
          ('rustflags', {'RUSTFLAGS': '-Cdebuginfo=1'}),
          ('profile', {'CARGO_PROFILE_RELEASE_DEBUG': '1'}))
# Probe a real path, never a bare host root: several of these answer 400 or 403
# at "/" while serving their actual content perfectly. Treating that as a denial
# sends the next reader chasing a phantom.
EGRESS_PROBES = (
    ('https://deb.debian.org/debian/dists/stable/Release', 'deb.debian.org',
     'Debian packages (libtkrzw-dev, libkyotocabinet-dev, glib, valgrind)'),
    ('https://production.cloudfront.docker.com/', 'cloudfront.docker.com',
     'Docker Hub blob CDN (base image layers)'),
    ('https://registry-1.docker.io/v2/', 'registry-1.docker.io', 'Docker Hub manifests'),
    ('https://mirror.gcr.io/v2/', 'mirror.gcr.io', 'a mirror that can serve the same base image layers'),
    ('https://static.rust-lang.org/rustup/archive/1.29.0/x86_64-unknown-linux-gnu/rustup-init.sha256',
     'static.rust-lang.org', 'rustup-init'),
    ('https://index.crates.io/config.json', 'index.crates.io', 'the cargo registry index'),
    ('https://static.crates.io/crates/libc/libc-0.2.189.crate', 'static.crates.io', 'crate downloads'),
    ('https://github.com/libpinyin/libpinyin.git/info/refs?service=git-upload-pack', 'github.com',
     'the pinned libpinyin and ibus-libpinyin trees'),
)
# Denial looks like exactly three things. 000 is "no HTTP response at all": DNS
# failure, refused, or a CONNECT rejected before any status line. 403/407 are the
# proxy refusing. A 401 (registries) or a 3xx (mirrors) means the host answered,
# which is all this asks.
DENIED_CODES = ('000', '403', '407')


def package_name(lines: Sequence[str]) -> str:
    """The first `name = "..."` value of a manifest, as `sed -n 's/^name = "\\(.*\\)"/\\1/p' | head -1` printed it.

    The text after the closing quote stays, as the substitution left it.
    """
    for line in lines:
        match = re.match('name = "(.*)"', line)
        if match:
            return match.group(1) + line[match.end():]
    return ''


def library_name(lines: Sequence[str]) -> str:
    """The `name` the [lib] section gives, which wins over the package name for the artifact's filename.

    A name line loses its quotes and commas and is read as words, the third being
    the value (`name = "pinyin"`); `name="pinyin"` is one word and yields nothing.
    """
    in_library, found = False, []
    for line in lines:
        if line.startswith('[lib]'):
            in_library = True
            continue
        if line.startswith('['):
            in_library = False
        if in_library and re.match('name *=', line):
            words = re.findall('[^ \t\n]+', re.sub('[",]', '', line))
            found.append(words[2] if len(words) > 2 else '')
    return ''.join(name + '\n' for name in found).rstrip('\n')


def debug_sections(listing: bytes) -> int:
    """The lines of a `readelf -S` listing that name a debug section (`grep -c debug_`)."""
    return sum(1 for line in neutrality.lines(listing) if 'debug_' in line)


def rustc_version(env: Mapping[str, str]) -> str:
    """What `rustc --version` prints, or nothing and a note when there is no rustc."""
    if shutil.which('rustc', path=env.get('PATH')) is None:
        emit('rustc: command not found', error=True)
        return ''
    done = bench.capture(['rustc', '--version'], env=env)
    if done.stderr:
        write(done.stderr, error=True)
    return done.stdout.decode('utf-8', 'replace').rstrip('\n')


def build_variant(tag: str, assignments: Mapping[str, str], *, package: str, backend: str, artifact: Path,
                  work: Path, env: Mapping[str, str]) -> None:
    """Build the crate with ASSIGNMENTS in the environment, keep the artifact as WORK/TAG.so and describe it.

    The build touches the crate's lib.rs first so cargo rebuilds it, prints nothing
    and, as before, does not hold the heavy slot: this is a manual probe, run
    by hand. A failed build ends the probe with its status and no message.
    """
    library = Path('crates', package, 'src', 'lib.rs')
    try:
        library.touch()
    except OSError as error:
        bench.fail(f"touch: cannot touch '{library}': {error.strerror}")
    if shutil.which('cargo', path=env.get('PATH')) is None:
        raise Finished(127)
    done = bench.capture(['cargo', 'build', '--locked', '--release', '-p', package,
                          '--no-default-features', '--features', backend], env={**env, **assignments})
    if done.status:
        raise Finished(done.status)
    kept = work / f'{tag}.so'
    try:
        shutil.copy(artifact, kept)
    except OSError as error:
        bench.fail(f"cp: cannot stat '{artifact}': {error.strerror}")
    sections = 0
    if shutil.which('readelf', path=env.get('PATH')):
        sections = debug_sections(bench.capture(['readelf', '-S', kept], env=env).stdout)
    size = f'{kept.stat().st_size} B'
    emit(f'{tag:<10} {size:<12} debug sections: {sections}')


def probe_debuginfo_neutrality(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Build the crate stock, with RUSTFLAGS debug info and with profile debug info, and classify the pair that matters."""
    backend = argv[0] if argv and argv[0] else 'bdb'
    package = env.get('PKG') or 'oxpinyin-capi'
    with tempfile.TemporaryDirectory() as directory, bench.working_directory(ROOT):
        work = Path(directory)
        emit(f'=== debug-info neutrality: {package}, backend {backend} ===')
        emit(f'captured_utc: {bench.utc_now()}')
        emit(f'rustc: {rustc_version(env)}')
        emit()

        manifest = Path('crates', package, 'Cargo.toml')
        try:
            lines = list(neutrality.lines(manifest.read_bytes()))
        except OSError as error:
            bench.fail(f"sed: can't read {manifest}: {error.strerror}", 2)
        artifact = Path('target/release') / f'lib{library_name(lines) or package_name(lines)}.so'

        # Serial on purpose: the three builds share target/release, and each
        # artifact is copied out before the next build replaces it.
        for tag, assignments in BUILDS:
            build_variant(tag, assignments, package=package, backend=backend, artifact=artifact,
                          work=work, env=env)
        emit()

        for tag, _ in BUILDS:
            bench.run(['objcopy', '-O', 'binary', '--only-section=.text', work / f'{tag}.so', work / f'{tag}.text'],
                      env=env)
        emit('=== .text ===')
        for tag, _ in BUILDS:
            text = (work / f'{tag}.text').read_bytes()
            emit(f'{tag:<10} {len(text):>10} B  sha256 {hashlib.sha256(text).hexdigest()[:16]}…')
        emit()

        if (work / 'stock.so').read_bytes() == (work / 'rustflags.so').read_bytes():
            emit('CONFIRMED: RUSTFLAGS=-Cdebuginfo=1 produced a byte-identical')
            emit('           artifact — cargo stripped it. Add -Cstrip=none to RUSTFLAGS.')
        else:
            emit('NOTE: RUSTFLAGS=-Cdebuginfo=1 did change the artifact on this')
            emit('      toolchain; the strip-back behaviour above may have been fixed.')
        emit()

        emit('=== verdict: stock vs profile-debug ===')
        status, report = neutrality.compare(work / 'stock.so', work / 'profile.so')
        write(report.encode('utf-8'))
        return status


def curl(arguments: Sequence[str], env: Mapping[str, str]) -> tuple[int, bytes]:
    """Run curl for a probe and return (status, standard output); its standard error is discarded.

    A missing curl is status 127 and nothing printed, as `2>/dev/null` left it.
    """
    if shutil.which('curl', path=env.get('PATH')) is None:
        return 127, b''
    done = bench.capture(['curl', *arguments], env=env)
    return done.status, done.stdout


def denials(status: bytes) -> list[str]:
    """The deduplicated `HOST KIND: DETAIL` rows of an agent proxy's recent relay failures.

    A reply that is not JSON has none. A reply of another shape ends the rows where
    it stops making sense; the shell script ended with an interpreter traceback there.
    """
    try:
        document = json.loads(status.decode('utf-8'))
    except Exception:
        return []
    seen, rows = set(), []
    try:
        for failure in document.get('recentRelayFailures', []):
            host = failure.get('host')
            if not host:
                continue
            key = (host, failure.get('kind'))
            if key in seen:
                continue
            seen.add(key)
            rows.append(f'  {host:<42} {failure.get("kind")}: {failure.get("detail")}')
    except (AttributeError, TypeError):
        pass
    return rows


def probe_egress(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Print whether each host a perf image build needs answers; exit 1 if any is denied."""
    def row(*cells) -> str:
        return '{:<24} {:<6} {:<8} {}'.format(*cells)

    emit(row('HOST', 'CODE', 'VERDICT', 'NEEDED-FOR'))
    emit(row('-----------------------', '-----', '-------', '----------'))
    failed = False
    # Serial on purpose: this is a diagnostic to run before anything heavy, and a
    # pool would hold the heavy slot, perhaps behind a build, to make eight short
    # requests whose table prints in a fixed order anyway.
    for url, host, need in EGRESS_PROBES:
        _, output = curl(['-s', '-o', '/dev/null', '-w', '%{http_code}', '--max-time', '20', url], env)
        code = output.decode('utf-8', 'replace').rstrip('\n')
        verdict = 'ok'
        if code in DENIED_CODES:
            verdict, failed = 'DENIED', True
        emit(row(host, code, verdict, need))

    # An agent proxy records why it rejected a CONNECT; curl cannot show it.
    proxy = env.get('HTTPS_PROXY')
    if proxy:
        emit()
        emit('--- proxy-recorded denials (deduplicated) ---')
        for line in denials(curl(['-sS', f'{proxy}/__agentproxy/status'], env)[1]):
            emit(line)

    emit()
    if failed:
        emit('RESULT: at least one required host is unreachable or denied.')
        emit('        A perf image build will fail. Report the blocked host;')
        emit('        do not route around an egress policy.')
        return 1
    emit('RESULT: every host a perf image build needs is reachable.')
    return 0


RECIPES = {'probe-debuginfo-neutrality': probe_debuginfo_neutrality, 'probe-egress': probe_egress}
