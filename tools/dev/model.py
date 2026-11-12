"""Fetch the checksum-pinned libpinyin model archive into a build cache.

The archive, its extraction and any converted form stay in the cache:
they must not enter the repository, a release or an installer
(docs/findings/model-provenance.md). Nothing here feeds frequencies into the
language model.

The expected archive SHA-256 comes from tools/oracle/oracle-pin.txt.

Network: downloads.sourceforge.net over HTTPS; SourceForge may redirect to a
mirror. There is no vendored copy and no alternate URL: model-provenance.md
classifies model20 as not redistributable, so no project-owned mirror exists.
No CI job downloads it. A developer machine fetches it once into the cache,
and tools/oracle/build.py reuses that cache through --model-dir.

Cache: model20-<the first 12 digits of the archive SHA-256>/ under the cache
root (tools/cachedir.py)
  downloads/model20.text.tar.gz
  extracted/{interpolation2.text + seventeen .table files}
  verified (the marker that records the URL and digest)

The last stdout line is the absolute path of the extracted directory.
"""

if __name__ == '__main__':
    from .entry import require_utf8_mode

    require_utf8_mode()

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

from .process import ROOT, fail, run
from .runners import guarded
from tools import cachedir
from tools.oracle.build import MODEL_FILES, fields

MODEL_URL = 'https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz'
PIN_FILE = ROOT / 'tools/oracle/oracle-pin.txt'
ARCHIVE_NAME = 'model20.text.tar.gz'
MARKER_SCHEMA = 'pinyin-model-cache-v1'
# The eighteen-file inventory; the guest builder owns it and cannot import tools.dev.
EXPECTED_MODEL_FILES = MODEL_FILES
FETCH_HINT = 'just fetch-model'

USAGE = """\
Usage: just fetch-model [OPTIONS]
       python -m tools.dev.model [OPTIONS]

Download the checksum-pinned model20.text.tar.gz, verify SHA-256, and
extract interpolation2.text plus the seventeen .table files into a build
cache. Idempotent: a verified extraction is left untouched.

Options:
  --url URL        Archive URL (default: the pinned SourceForge URL, or
                   $PINYIN_MODEL_URL)
  --archive PATH   Use a local archive instead of downloading
  --force          Re-download (if needed) and re-extract even if verified
  -h, --help       Show this help

The cache is the model20 entry under $OXPINYIN_CACHE. It must be a gitignored
path inside the work tree, or a path outside the repository. The last stdout
line is the extracted directory.
"""


def file_sha256(path):
    """Return the hex SHA-256 of PATH."""
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def extraction_complete(directory):
    """True when DIRECTORY holds every expected model file."""
    directory = Path(directory)
    return directory.is_dir() and all((directory / name).is_file() for name in EXPECTED_MODEL_FILES)


def archive_sha256():
    """The expected SHA-256 of the model archive in this checkout."""
    return fields(PIN_FILE)['model_archive_sha256']


def default_cache(env=None):
    """The model20 entry of the cache root, named by the pinned archive digest."""
    return cachedir.root(env) / f'model20-{archive_sha256()[:12]}'


def marker_matches(marker, digest=None):
    """True when MARKER has the line `sha256=<pinned digest>`."""
    if digest is None:
        digest = archive_sha256()
    try:
        return f'sha256={digest}'.encode() in Path(marker).read_bytes().splitlines()
    except OSError:
        return False


def _inside(path, directory):
    return path == directory or directory in path.parents


def _tool(name, env):
    """The program that NAME names on the PATH of ENV, or None.

    Windows starts a bare command name through the PATH of this process, not the PATH of the
    environment it is given. The path found here is the one to run, on every host.
    """
    return shutil.which(name, path=env.get('PATH'))


def _git_vets(root, env):
    """The git that can answer ignore questions for ROOT's work tree, or None."""
    git = _tool('git', env)
    if git is None:
        return None
    vets = run([git, 'rev-parse', '--is-inside-work-tree'], cwd=root, env=env, stdout=subprocess.DEVNULL,
               stderr=subprocess.DEVNULL, check=False) == 0
    return git if vets else None


def refuse_tracked(cache, root, env):
    """End the run when CACHE lies in ROOT where git would track model bytes."""
    if not _inside(cache, root):
        return
    git = _git_vets(root, env)
    if git:
        if run([git, 'check-ignore', '-q', '--', cache], cwd=root, env=env, stdout=subprocess.DEVNULL,
               stderr=subprocess.DEVNULL, check=False):
            fail(f'refusing to write model bytes to a path git would track: {cache}\n'
                 'set OXPINYIN_CACHE to an ignored path or a path outside the repository')
    elif not _inside(cache, root / 'target'):
        fail(f'refusing to write model bytes under the repository without git ignore checks: {cache}')


def download(url, destination, env):
    """Download URL to a private staging path."""
    curl = _tool('curl', env)
    if curl is None:
        fail('required command not found: curl')
    print(f'downloading {url}', file=sys.stderr, flush=True)
    status = run([curl, '--fail', '--location', '--retry', '3', '--connect-timeout', '30',
                  '--max-time', '600', '--show-error', '--output', destination, url], env=env, check=False)
    if status:
        fail(f'failed to download {ARCHIVE_NAME} from {url}')


def extract(source, destination):
    """Extract SOURCE into the private DESTINATION and check its inventory."""
    try:
        with tarfile.open(source, 'r:gz') as archive:
            archive.extractall(destination, filter='data')
    except (tarfile.TarError, OSError, EOFError):
        fail(f'failed to extract {source}')
    missing = [name for name in EXPECTED_MODEL_FILES if not (destination / name).is_file()]
    if missing:
        fail(''.join(f'extracted archive is missing {name}\n' for name in missing).rstrip('\n'))


def publish(staging, extracted, marker, backup):
    """Install a complete tree and marker while holding the fetch lock."""
    had_extraction = os.path.lexists(extracted)
    if had_extraction:
        extracted.rename(backup)
    tree = staging / 'extracted'
    try:
        tree.rename(extracted)
        (staging / 'verified').replace(marker)
    except OSError:
        if not tree.exists():
            extracted.rename(tree)
        if had_extraction:
            backup.rename(extracted)
        raise
    if had_extraction:
        backup.rename(staging / 'previous')


def fetch(cache_dir=None, *, url=None, archive=None, force=False, root=ROOT, env=None):
    """Return the verified extraction under CACHE_DIR, fetching and extracting as needed.

    CACHE_DIR defaults to default_cache. URL defaults to $PINYIN_MODEL_URL in ENV
    (an empty value counts as unset), then to the pin.
    Every failure prints its diagnostic and raises Finished.
    """
    env = dict(os.environ if env is None else env)
    root = Path(os.path.abspath(root))
    cache = Path(os.path.abspath(cache_dir or default_cache(env)))
    url = url or env.get('PINYIN_MODEL_URL') or MODEL_URL
    refuse_tracked(cache, root, env)
    with cachedir.exclusive_fetch(cache):
        return _fetch_locked(cache, url, archive, force, env)


def _fetch_locked(cache, url, archive, force, env):
    extracted = cache / 'extracted'
    marker = cache / 'verified'
    backup = cache / '.extracted-backup'
    if os.path.lexists(backup):
        fail(f'previous model publication needs recovery: {backup} (see docs/runbooks/setup.md#cache-directory)')
    digest = archive_sha256()
    if not force and extraction_complete(extracted) and marker_matches(marker, digest):
        print('verified model cache present; skipping fetch', file=sys.stderr)
        return extracted
    if archive and not Path(archive).is_file():
        fail(f'local archive is not a file: {archive}')
    cache.mkdir(exist_ok=True)
    downloads = cache / 'downloads'
    cached_archive = downloads / ARCHIVE_NAME
    with tempfile.TemporaryDirectory(prefix='.extract.', dir=cache) as directory:
        staging = Path(directory)
        source = Path(archive) if archive else cached_archive
        downloaded = not archive and (force or not source.is_file())
        if downloaded:
            source = staging / ARCHIVE_NAME
            download(url, source, env)
        actual = file_sha256(source)
        if actual != digest:
            fail(f'checksum mismatch: {ARCHIVE_NAME}\n  expected: {digest}\n  actual:   {actual}')
        extract(source, staging / 'extracted')
        (staging / 'verified').write_text(f'schema={MARKER_SCHEMA}\nurl={MODEL_URL}\nsha256={digest}\n')
        if downloaded:
            downloads.mkdir(exist_ok=True)
            source.replace(cached_archive)
        publish(staging, extracted, marker, backup)
    return extracted


def main(argv=None, env=None):
    """Parse the fetch options, run the fetch and print the extracted directory."""
    args = list(sys.argv[1:] if argv is None else argv)
    env = dict(os.environ if env is None else env)
    options = {'url': None, 'archive': None, 'force': False}
    valued = {'--url': 'url', '--archive': 'archive'}
    while args:
        option = args.pop(0)
        if option in ('-h', '--help'):
            print(USAGE, end='')
            return 0
        if option == '--force':
            options['force'] = True
        elif option in valued and args:
            options[valued[option]] = args.pop(0)
        else:
            message = f'option needs a value: {option}' if option in valued else f'unknown option: {option}'
            print(message, file=sys.stderr)
            print(USAGE, end='', file=sys.stderr)
            return 2

    def call():
        print(fetch(env=env, **options))
        return 0
    return guarded(call)


if __name__ == '__main__':
    raise SystemExit(main())
