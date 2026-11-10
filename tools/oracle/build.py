#!/usr/bin/env python3
"""Build the pinned libpinyin reference (the oracle) inside its Linux environment.

Standard library only, and no tools.dev import: Dockerfile.oracle's build stage runs this
file with Debian's python3 before any managed interpreter is used. Host tooling on Windows,
macOS and Linux starts it in a container (tools/dev/provision.py, tools/oracle/image.py);
upstream's own git, autoreconf, configure, make and patch do the fetching and compiling.

tools/oracle/oracle-pin.txt, beside this file, is the only record of the pinned commits,
version and model digest. Both upstreams are fetched and verified by commit SHA: no release
tag carries the libpinyin pin, and a commit names the git tree where an archive digest names
one tarball. The model archive is a plain download and stays on its SHA-256.

The last stdout line is the absolute path of the built libpinyin shared object.
"""
import argparse
import hashlib
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
# The files whose bytes, concatenated in this order, are the recipe identity that every
# prefix records as recipe_sha256. tools.oracle.image reads this same list.
RECIPE_INPUTS = ('Dockerfile.oracle', 'build.py', 'version-manifest.json', 'versions.py', 'oracle-pin.txt')
LIBPINYIN_GIT_URL = 'https://github.com/libpinyin/libpinyin.git'
IBUS_LIBPINYIN_GIT_URL = 'https://github.com/libpinyin/ibus-libpinyin.git'
MODEL_URL = 'https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz'
MODEL_ARCHIVE = 'model20.text.tar.gz'
# The model20 export inventory (docs/findings/model-provenance.md); tools/dev/model.py imports it.
MODEL_FILES = (
    'art.table', 'culture.table', 'economy.table', 'gb_char.table', 'gbk_char.table',
    'geology.table', 'history.table', 'interpolation2.text', 'life.table', 'merged.table',
    'nature.table', 'opengram.table', 'people.table', 'punct.table', 'science.table',
    'society.table', 'sport.table', 'technology.table')
# --dbm value (also the pin ref's +dbm- suffix) to the name libpinyin's configure takes.
DBM_NAMES = {'tkrzw': 'Tkrzw', 'kc': 'KyotoCabinet', 'bdb': 'BerkeleyDB'}
DATA_DIRECTORY = 'lib/libpinyin/data'
# Two clean builds of one pin in one image differ in exactly these six files, which the
# DBM-backed generation path writes. They get their own manifest; neither manifest is frozen
# across compiler, libc or DBM updates.
DATA_UNSTABLE_FILES = ('addon_phrase_index.bin', 'addon_pinyin_index.bin', 'bigram.db',
                       'phrase_index.bin', 'pinyin_index.bin', 'punct.bin')
IMAGE_VERSIONS = Path('/opt/oracle-versions.txt')
# Every compiler-recorded source path is rewritten to this name, so the libraries do not
# depend on --work-dir.
SOURCE_NAME = '/usr/src/libpinyin'


class BuildError(Exception):
    """A refusal with its exit status: 2 for usage, 1 for everything else."""

    def __init__(self, message, status=1):
        super().__init__(message)
        self.status = status


def fields(path):
    """Read a key=value record, skipping comments."""
    return dict(line.split('=', 1) for line in Path(path).read_text().splitlines()
                if line and not line.startswith('#') and '=' in line)


def recipe_hash(root=HERE.parents[1]):
    """SHA-256 of RECIPE_INPUTS' bytes under ROOT/tools/oracle, in order."""
    digest = hashlib.sha256()
    for name in RECIPE_INPUTS:
        digest.update((Path(root) / 'tools/oracle' / name).read_bytes())
    return digest.hexdigest()


def default_dbm(pin):
    """The --dbm value of the pin record's dbm field."""
    return next(key for key, name in DBM_NAMES.items() if name == pin['dbm'])


def file_sha256(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def run(command, **options):
    """Run one build command; a failure raises CalledProcessError."""
    return subprocess.run([str(part) for part in command], check=True, **options)


def output(command, **options):
    return run(command, stdout=subprocess.PIPE, text=True, **options).stdout.strip()


def positive(value):
    if not (value.isascii() and value.isdecimal() and int(value) > 0):
        raise argparse.ArgumentTypeError('--jobs must be a positive integer')
    return value


def parser(pin):
    result = argparse.ArgumentParser(
        prog='build.py', allow_abbrev=False, formatter_class=argparse.RawDescriptionHelpFormatter,
        description='Build the pinned libpinyin and the pinned ibus-libpinyin, each fetched by commit SHA\n'
                    '(git fetch --depth=1) and verified by git rev-parse.',
        epilog='CC, CXX, CFLAGS, CXXFLAGS and LDFLAGS are passed through. The library build is reproducible:\n'
               "SOURCE_DATE_EPOCH is the pin's commit time, the source path is remapped to a fixed name and\n"
               'the ELF build ID is omitted, so the libraries hash the same wherever the work directory is,\n'
               'given the same toolchain and --prefix (python -m tools.oracle.image check-reproducible).\n'
               'The last stdout line is the absolute path of the built libpinyin shared object.')
    result.add_argument('--work-dir', metavar='DIR',
                        help='download and build directory (default: $TMPDIR/oxpinyin-oracle)')
    result.add_argument('--prefix', metavar='DIR', help='installation prefix (default: WORK_DIR/prefix)')
    result.add_argument('--jobs', metavar='N', type=positive, default='1', help='parallel make jobs (default: 1)')
    result.add_argument('--apply-patches', metavar='DIR',
                        help='apply every *.patch in DIR, in name order, to the libpinyin source before '
                             'autoreconf; their checksums go into oracle-patches.sha256 and the pin ref. '
                             'Needs an explicit --prefix')
    result.add_argument('--model-dir', metavar='DIR',
                        help='use the verified model export that just fetch-model extracted instead of '
                             'downloading the pinned archive; DIR/../verified must record the pinned SHA-256')
    result.add_argument('--enable-libzhuyin', action='store_true', help='also build and install libzhuyin')
    result.add_argument('--dbm', metavar='NAME', default=default_dbm(pin),
                        help=f'DBM backend: {", ".join(DBM_NAMES)} (default: {default_dbm(pin)}); recorded in '
                             'the pin ref (+dbm-NAME) and the dbm field')
    result.add_argument('--libs-only', action='store_true',
                        help='build only the libraries, headers, pkg-config files and oracle-pin.txt: no model, '
                             'no ibus-libpinyin, no data. This is what the oracle image carries')
    result.add_argument('--regen-data', action='store_true',
                        help="into an existing --libs-only prefix of this pin, recipe and dbm, build the pin's "
                             'data from the model in a staging area beside the prefix, then publish the data '
                             'and its manifests together')
    return result


def options(argv, pin):
    """Parse ARGV and apply the usage rules; every refusal here is status 2."""
    args = parser(pin).parse_args(argv)
    if args.libs_only and args.regen_data:
        raise BuildError('--libs-only and --regen-data are mutually exclusive', 2)
    if args.apply_patches and not args.prefix:
        raise BuildError('--apply-patches requires an explicit --prefix: a patched build must not land in '
                         'the unpatched default prefix', 2)
    if args.dbm not in DBM_NAMES:
        raise BuildError('--dbm must be one of: ' + ', '.join(DBM_NAMES), 2)
    if args.regen_data and args.apply_patches:
        raise BuildError('--regen-data cannot be combined with --apply-patches', 2)
    args.mode = 'libs' if args.libs_only else 'data' if args.regen_data else 'full'
    args.work_dir = Path(args.work_dir or Path(os.environ.get('TMPDIR') or tempfile.gettempdir()) / 'oxpinyin-oracle')
    args.prefix = Path(args.prefix or args.work_dir / 'prefix')
    return args


def check_prefix(prefix, mode, pin_ref, recipe):
    """A data build needs a libraries prefix of this pin, recipe and dbm; any other needs an empty one."""
    if prefix.exists() and not prefix.is_dir():
        raise BuildError(f'installation prefix is not a directory: {prefix}')
    if mode == 'data':
        try:
            record = (prefix / 'oracle-pin.txt').read_text().splitlines()
        except OSError:
            record = []
        if f'pin_ref={pin_ref}' not in record or f'recipe_sha256={recipe}' not in record:
            raise BuildError(f'FAIL: {prefix} is not a libs-only prefix of this pin, recipe and dbm\n'
                             f'  expected pin_ref={pin_ref} in {prefix}/oracle-pin.txt')
    elif prefix.is_dir() and any(prefix.iterdir()):
        raise BuildError(f'installation prefix must be empty: {prefix}')


def check_model_dir(model_dir, digest):
    """MODEL_DIR holds the whole inventory and the marker above it records the pinned archive.

    Without the marker the bytes could be any eighteen files with the right names."""
    if not model_dir.is_dir():
        raise BuildError(f'--model-dir is not a directory: {model_dir}')
    missing = [name for name in MODEL_FILES if not (model_dir / name).is_file()]
    if missing:
        raise BuildError('FAIL: --model-dir is missing: ' + ' '.join(missing) +
                         '\nDIR must hold the verified model20 export (run just fetch-model)')
    marker = model_dir.parent / 'verified'
    try:
        recorded = marker.read_text().splitlines()
    except (OSError, UnicodeError):
        recorded = []
    if f'sha256={digest}' not in recorded:
        raise BuildError('FAIL: --model-dir lacks a matching provenance marker\n'
                         f'  expected sha256={digest} in {marker}\n'
                         '  run just fetch-model to produce a verified cache')


def fetch_commit(name, url, commit, work_dir):
    """A fresh shallow checkout of COMMIT, named by the commit, with HEAD verified against it."""
    source = work_dir / 'src' / f'{name}-{commit}'
    shutil.rmtree(source, ignore_errors=True)
    source.mkdir(parents=True)
    run(['git', 'init', '--quiet', source])
    try:
        run(['git', '-C', source, 'fetch', '--quiet', '--depth=1', url, commit])
    except subprocess.CalledProcessError:
        raise BuildError(f'git fetch of {name} {commit} failed') from None
    run(['git', '-C', source, 'checkout', '--quiet', '--detach', 'FETCH_HEAD'])
    fetched = output(['git', '-C', source, 'rev-parse', 'HEAD'])
    if fetched != commit:
        raise BuildError(f'{name} commit mismatch: fetched {fetched}, expected {commit}')
    return source


def download_model(work_dir, digest, destination):
    """Download the pinned archive unless cached, verify it, and extract only the inventory."""
    archive = work_dir / 'downloads' / MODEL_ARCHIVE
    if not archive.is_file():
        if shutil.which('curl') is None:
            raise BuildError('required command not found: curl')
        run(['curl', '--fail', '--location', '--retry', '3', '--show-error', MODEL_URL, '--output', archive])
    if file_sha256(archive) != digest:
        raise BuildError(f'checksum mismatch: {MODEL_ARCHIVE}')
    # Only regular members named by the inventory are read, each to its bare name: no member
    # path, link or device from the archive reaches the filesystem.
    with tempfile.TemporaryDirectory(dir=work_dir) as scratch:
        try:
            with tarfile.open(archive, 'r:gz') as tar:
                for member in tar:
                    name = member.name.removeprefix('./')
                    if member.isreg() and name in MODEL_FILES:
                        with tar.extractfile(member) as stream, (Path(scratch) / name).open('wb') as target:
                            shutil.copyfileobj(stream, target)
        except (tarfile.TarError, OSError, EOFError):
            raise BuildError(f'failed to extract {archive}') from None
        copy_model(Path(scratch), destination, 'the pinned archive')


def copy_model(source, destination, origin):
    """Copy the validated inventory only: a stray file beside it must not enter the oracle's data."""
    missing = [name for name in MODEL_FILES if not (source / name).is_file()]
    if missing:
        raise BuildError(f'FAIL: {origin} is missing: ' + ' '.join(missing))
    destination.mkdir(parents=True, exist_ok=True)
    for name in MODEL_FILES:
        shutil.copyfile(source / name, destination / name)


def apply_patches(directory, source, prefix):
    """Apply DIRECTORY's patches in name order; return the SHA-256 of the manifest written to PREFIX."""
    if shutil.which('patch') is None:
        raise BuildError('required command not found: patch')
    if not directory.is_dir():
        raise BuildError(f'--apply-patches: not a directory: {directory}')
    patches = sorted((path for path in directory.iterdir()
                      if path.name.endswith('.patch') and path.is_file() and not path.is_symlink()),
                     key=lambda path: path.name)
    if not patches:
        raise BuildError(f'--apply-patches: no *.patch files in {directory}')
    for patch in patches:
        print(f'applying patch: {patch}', file=sys.stderr, flush=True)
        with patch.open('rb') as stream:
            run(['patch', '-p1', '--forward', '--no-backup-if-mismatch'], cwd=source, stdin=stream)
    manifest = prefix / 'oracle-patches.sha256'
    manifest.write_bytes(''.join(f'{file_sha256(patch)}  ./{patch.name}\n' for patch in patches).encode())
    return file_sha256(manifest)


def build_environment(source, prefix):
    """The reference recipe's environment: reproducible, and searching only its own prefix.

    PKG_CONFIG_PATH keeps an inherited base after the prefix (a from-source libtkrzw under
    /usr/local, which not every pkg-config searches). LD_LIBRARY_PATH is overwritten. CFLAGS and
    CXXFLAGS fall back to autoconf's -g -O2 only when unset, so the optimisation level is the bare
    ./configure build's. The build ID would hash path-bearing layout, so it is omitted."""
    env = dict(os.environ)
    inherited = env.get('PKG_CONFIG_PATH')
    env['PKG_CONFIG_PATH'] = f'{prefix}/lib/pkgconfig:{prefix}/lib64/pkgconfig' + (f':{inherited}' if inherited else '')
    env['LD_LIBRARY_PATH'] = f'{prefix}/lib:{prefix}/lib64'
    env['SOURCE_DATE_EPOCH'] = output(['git', '-C', source, 'log', '-1', '--format=%ct'])
    env['TZ'], env['LC_ALL'] = 'UTC', 'C'
    remap = f'-ffile-prefix-map={source}={SOURCE_NAME} -fdebug-prefix-map={source}={SOURCE_NAME}'
    for name in ('CFLAGS', 'CXXFLAGS'):
        env[name] = f'{env.get(name, "-g -O2")} {remap}'
    env['LDFLAGS'] = (env['LDFLAGS'] + ' ' if env.get('LDFLAGS') else '') + '-Wl,--build-id=none'
    return env


def build_libpinyin(source, env, args, stage):
    """autoreconf, configure and the mode's make targets."""
    def step(*command):
        run(command, cwd=source, env=env)

    step('autoreconf', '--force', '--install', '--verbose')
    step('./configure', f'--prefix={args.prefix}', '--disable-static', f'--with-dbm={DBM_NAMES[args.dbm]}',
         *(['--enable-libzhuyin'] if args.enable_libzhuyin else []))
    if args.mode == 'full':
        step('make', f'-j{args.jobs}')
        step('make', 'install')
    elif args.mode == 'libs':
        # src/ holds both libraries and the installed headers; the .pc files are the top-level
        # pkgconfig_DATA. Nothing here reads data/.
        step('make', '-C', 'src', f'-j{args.jobs}')
        step('make', '-C', 'src', 'install')
        step('make', 'install-pkgconfigDATA')
    else:
        # Everything is built so utils/ links the fresh libraries, but only data/ and the
        # data-build utilities are installed, into the staging area: the prefix keeps the
        # libraries it was provisioned with. DESTDIR keeps the real prefix in run paths.
        step('make', f'-j{args.jobs}')
        step('make', '-C', 'utils', 'install', f'DESTDIR={stage}')
        step('make', '-C', 'data', 'install', f'DESTDIR={stage}')


def build_ibus(source, env, args):
    for command in (['autoreconf', '--force', '--install', '--verbose'],
                    ['./configure', f'--prefix={args.prefix}', '--disable-static', '--disable-boost',
                     '--disable-cloud-input-mode', '--disable-english-input-mode', '--disable-libnotify',
                     '--disable-lua-extension'],
                    ['make', f'-j{args.jobs}']):
        run(command, cwd=source, env=env)


def shared_object(prefix):
    """The first regular libpinyin.so or libpinyin.so.* file under PREFIX, in path order."""
    found = sorted(str(Path(directory) / name) for directory, _, names in os.walk(prefix) for name in names
                   if (name == 'libpinyin.so' or name.startswith('libpinyin.so.'))
                   and not (Path(directory) / name).is_symlink())
    if not found:
        raise BuildError('built libpinyin shared object not found')
    return found[0]


def data_manifests(content_root, out_dir):
    """Write both data manifests for CONTENT_ROOT into OUT_DIR; return their SHA-256s.

    Lines are sha256sum's: digest, two spaces, the path relative to the prefix, in path order."""
    data = content_root / DATA_DIRECTORY
    if not data.is_dir():
        raise BuildError('installed oracle data directory not found')
    unstable = [f'{DATA_DIRECTORY}/{name}' for name in DATA_UNSTABLE_FILES]
    for name in unstable:
        if not (content_root / name).is_file():
            raise BuildError(f'generated data file not found: {Path(name).name}')
    stable = sorted({path.relative_to(content_root).as_posix() for path in data.rglob('*')
                     if path.is_file() and not path.is_symlink()} - set(unstable))
    digests = []
    for manifest, names in (('oracle-data.sha256', stable), ('oracle-data-unstable.sha256', unstable)):
        (out_dir / manifest).write_bytes(''.join(f'{file_sha256(content_root / name)}  {name}\n' for name in names).encode())
        digests.append(file_sha256(out_dir / manifest))
    return digests


def record_versions(args, out_dir):
    """versions.txt: the libraries' own record for a data build, else the image's, else this host's."""
    target = out_dir / 'versions.txt'
    if args.mode == 'data':
        shutil.copyfile(args.prefix / 'versions.txt', target)
    elif IMAGE_VERSIONS.is_file():
        shutil.copyfile(IMAGE_VERSIONS, target)
    else:
        run([sys.executable, HERE / 'versions.py', '--print', '--record', target])
    return file_sha256(target)


def build(args, pin):
    """Run one build; return the built shared object's path."""
    model_digest = pin['model_archive_sha256']
    version, commit = pin['libpinyin_tag'], pin['libpinyin_commit']
    pin_ref = f'libpinyin-{version}-{commit}+model20-{model_digest}+dbm-{args.dbm}'
    recipe = recipe_hash()
    for command in ('git', 'autoreconf', 'make', 'pkg-config'):
        if shutil.which(command) is None:
            raise BuildError(f'required command not found: {command}')
    check_prefix(args.prefix, args.mode, pin_ref, recipe)
    if args.model_dir:
        check_model_dir(Path(args.model_dir), model_digest)
    for directory in (args.work_dir / 'downloads', args.work_dir / 'src', args.prefix):
        directory.mkdir(parents=True, exist_ok=True)
    args.work_dir, args.prefix = Path(os.path.abspath(args.work_dir)), Path(os.path.abspath(args.prefix))
    prefix = args.prefix

    source = fetch_commit('libpinyin', LIBPINYIN_GIT_URL, commit, args.work_dir)
    ibus = (fetch_commit('ibus-libpinyin', IBUS_LIBPINYIN_GIT_URL, pin['ibus_libpinyin_commit'], args.work_dir)
            if args.mode == 'full' else None)
    env = build_environment(source, prefix)
    if args.mode != 'libs':
        if args.model_dir:
            model_dir = Path(os.path.abspath(args.model_dir))
            copy_model(model_dir, source / 'data', '--model-dir')
            print(f'reusing pre-extracted model export from {model_dir}', file=sys.stderr)
        else:
            download_model(args.work_dir, model_digest, source / 'data')
    patches = apply_patches(Path(os.path.abspath(args.apply_patches)), source, prefix) if args.apply_patches else ''
    if patches:
        pin_ref += f'+patches-{patches}'

    # A data build generates beside the prefix, on its filesystem, and touches the prefix only
    # once the data, manifests and pin record all exist: a failed run leaves it as it was.
    stage = Path(tempfile.mkdtemp(prefix=prefix.name + '.stage.', dir=prefix.parent)) if args.mode == 'data' else None
    try:
        content_root = stage / prefix.relative_to(prefix.anchor) if stage else prefix
        out_dir = stage / 'out' if stage else prefix
        out_dir.mkdir(exist_ok=True)
        build_libpinyin(source, env, args, stage)
        if ibus:
            build_ibus(ibus, env, args)
        library = shared_object(prefix)
        header = prefix / f'include/libpinyin-{version}/pinyin.h'
        if not header.is_file():
            raise BuildError('installed oracle header not found')
        record = {
            'schema': 'pinyin-oracle-v1', 'pin_ref': pin_ref, 'recipe_sha256': recipe,
            'architecture': platform.machine(), 'versions_sha256': record_versions(args, out_dir),
            # libpinyin_tag keeps its schema name; its value is the version configure.ac reports.
            'libpinyin_tag': version, 'libpinyin_commit': commit,
            'ibus_libpinyin_tag': pin['ibus_libpinyin_tag'], 'ibus_libpinyin_commit': pin['ibus_libpinyin_commit'],
            'model_sha256': model_digest, 'dbm': DBM_NAMES[args.dbm],
            'header_sha256': file_sha256(header), 'shared_object_sha256': file_sha256(library)}
        if (prefix / 'lib/libzhuyin.so').is_file():
            record['zhuyin_shared_object_sha256'] = file_sha256(prefix / 'lib/libzhuyin.so')
        if args.mode != 'libs':
            record['data_manifest_sha256'], record['data_unstable_manifest_sha256'] = data_manifests(content_root, out_dir)
        if patches:
            record['patches_manifest_sha256'] = patches
        new = out_dir / 'oracle-pin.txt.new'
        new.write_bytes(''.join(f'{key}={value}\n' for key, value in record.items()).encode())
        new.replace(out_dir / 'oracle-pin.txt')
        if stage:
            shutil.rmtree(prefix / DATA_DIRECTORY, ignore_errors=True)
            shutil.copytree(content_root, prefix, symlinks=True, dirs_exist_ok=True)
            for name in ('oracle-data.sha256', 'oracle-data-unstable.sha256', 'oracle-pin.txt', 'versions.txt'):
                (out_dir / name).replace(prefix / name)
    finally:
        if stage:
            shutil.rmtree(stage, ignore_errors=True)
    print(f'libpinyin_tag={version}\nlibpinyin_commit={commit}\n'
          f'ibus_libpinyin_tag={pin["ibus_libpinyin_tag"]}\nibus_libpinyin_commit={pin["ibus_libpinyin_commit"]}',
          file=sys.stderr)
    return library


def main(argv=None):
    try:
        pin = fields(HERE / 'oracle-pin.txt')
        library = build(options(argv, pin), pin)
    except BuildError as error:
        print(error, file=sys.stderr)
        return error.status
    except subprocess.CalledProcessError as error:
        print(f'command failed with status {error.returncode}: {" ".join(error.cmd)}', file=sys.stderr)
        return error.returncode if error.returncode > 0 else 1
    sys.stdout.flush()
    print(library, flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
