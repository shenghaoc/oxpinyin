"""Fetch the checksum-pinned libpinyin model archive into a build cache.

The archive, its extraction and any converted form stay in an ignored cache:
they must not enter the repository, a release or an installer
(docs/findings/model-provenance.md). Nothing here feeds frequencies into the
language model.

Pinned identity (do not alter; from model-provenance.md):
  URL     https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz
  SHA-256 59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155

Network: downloads.sourceforge.net over HTTPS; SourceForge may redirect to a
mirror. There is no vendored copy and no alternate URL: model-provenance.md
classifies model20 as not redistributable, so no project-owned mirror exists.
No CI job downloads it. A developer machine fetches it once into the cache,
and build-oracle.sh reuses that cache through --model-dir.

Default cache: <repo>/target/model20/
  downloads/model20.text.tar.gz
  extracted/{interpolation2.text + seventeen .table files}
  verified (the marker that records the URL and digest)

The last stdout line is the absolute path of the extracted directory.
"""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

from .compat.process import ROOT, fail, run
from .runners import guarded

MODEL_URL = 'https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz'
MODEL20_SHA256 = '59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155'
ARCHIVE_NAME = 'model20.text.tar.gz'
MARKER_SCHEMA = 'pinyin-model-cache-v1'
# Inventory from docs/findings/model-provenance.md (eighteen files).
EXPECTED_MODEL_FILES = (
    'art.table', 'culture.table', 'economy.table', 'gb_char.table', 'gbk_char.table',
    'geology.table', 'history.table', 'interpolation2.text', 'life.table', 'merged.table',
    'nature.table', 'opengram.table', 'people.table', 'punct.table', 'science.table',
    'society.table', 'sport.table', 'technology.table')
FETCH_HINT = 'just fetch-model'

USAGE = """\
Usage: just fetch-model [OPTIONS]
       python -m tools.dev fetch-model [OPTIONS]

Download the checksum-pinned model20.text.tar.gz, verify SHA-256, and
extract interpolation2.text plus the seventeen .table files into a build
cache. Idempotent: a verified extraction is left untouched.

Options:
  --cache-dir DIR  Build cache (default: <repo>/target/model20, or
                   $PINYIN_MODEL_CACHE)
  --url URL        Archive URL (default: the pinned SourceForge URL, or
                   $PINYIN_MODEL_URL)
  --archive PATH   Use a local archive instead of downloading
  --force          Re-download (if needed) and re-extract even if verified
  -h, --help       Show this help

The cache must be a gitignored path inside the work tree, or a path
outside the repository. The last stdout line is the extracted directory.
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


def marker_matches(marker):
    """True when MARKER has the line `sha256=<pinned digest>`."""
    try:
        return f'sha256={MODEL20_SHA256}'.encode() in Path(marker).read_bytes().splitlines()
    except OSError:
        return False


def _inside(path, directory):
    return path == directory or directory in path.parents


def _git_vets(root, env):
    """True when git can answer ignore questions for ROOT's work tree."""
    if shutil.which('git', path=env.get('PATH')) is None:
        return False
    return run(['git', 'rev-parse', '--is-inside-work-tree'], cwd=root, env=env, stdout=subprocess.DEVNULL,
               stderr=subprocess.DEVNULL, check=False) == 0


def refuse_tracked(cache, root, env):
    """End the run when CACHE lies in ROOT where git would track model bytes."""
    if not _inside(cache, root):
        return
    if _git_vets(root, env):
        if run(['git', 'check-ignore', '-q', '--', cache], cwd=root, env=env, stdout=subprocess.DEVNULL,
               stderr=subprocess.DEVNULL, check=False):
            fail(f'refusing to write model bytes to a path git would track: {cache}\n'
                 'use a build cache under target/ (the default) or another ignored path')
    elif not _inside(cache, root / 'target'):
        fail(f'refusing to write model bytes under the repository without git ignore checks: {cache}')


def download(url, destination, env):
    """Download URL to DESTINATION through a .part file."""
    if shutil.which('curl', path=env.get('PATH')) is None:
        fail('required command not found: curl')
    print(f'downloading {url}', file=sys.stderr, flush=True)
    part = destination.with_name(destination.name + '.part')
    status = run(['curl', '--fail', '--location', '--retry', '3', '--connect-timeout', '30',
                  '--max-time', '600', '--show-error', '--output', part, url], env=env, check=False)
    if status:
        part.unlink(missing_ok=True)
        fail(f'failed to download {ARCHIVE_NAME} from {url}')
    part.replace(destination)


def extract(source, cache, extracted):
    """Extract SOURCE beside EXTRACTED, check the inventory, then move it into place."""
    cache.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix='.extract.', dir=cache))
    try:
        try:
            with tarfile.open(source, 'r:gz') as archive:
                archive.extractall(staging, filter='data')
        except (tarfile.TarError, OSError, EOFError):
            fail(f'failed to extract {source}')
        missing = [name for name in EXPECTED_MODEL_FILES if not (staging / name).is_file()]
        if missing:
            fail(''.join(f'extracted archive is missing {name}\n' for name in missing).rstrip('\n'))
        if extracted.is_dir() and not extracted.is_symlink():
            shutil.rmtree(extracted)
        elif extracted.exists() or extracted.is_symlink():
            extracted.unlink()
        staging.rename(extracted)
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def fetch(cache_dir=None, *, url=None, archive=None, force=False, root=ROOT, env=None):
    """Return the verified extraction under CACHE_DIR, fetching and extracting as needed.

    CACHE_DIR and URL default to $PINYIN_MODEL_CACHE and $PINYIN_MODEL_URL in ENV
    (an empty value counts as unset), then to <root>/target/model20 and the pin.
    Every failure prints its diagnostic and raises Finished.
    """
    env = dict(os.environ if env is None else env)
    root = Path(os.path.abspath(root))
    cache = Path(os.path.abspath(cache_dir or env.get('PINYIN_MODEL_CACHE') or root / 'target/model20'))
    url = url or env.get('PINYIN_MODEL_URL') or MODEL_URL
    extracted = cache / 'extracted'
    marker = cache / 'verified'
    refuse_tracked(cache, root, env)
    if not force and extraction_complete(extracted) and marker_matches(marker):
        print('verified model cache present; skipping fetch', file=sys.stderr)
        return extracted
    if archive:
        if not Path(archive).is_file():
            fail(f'local archive is not a file: {archive}')
        source = Path(os.path.abspath(archive))
    else:
        downloads = cache / 'downloads'
        downloads.mkdir(parents=True, exist_ok=True)
        source = downloads / ARCHIVE_NAME
        if force:
            source.unlink(missing_ok=True)
        if not source.is_file():
            download(url, source, env)
    actual = file_sha256(source)
    if actual != MODEL20_SHA256:
        fail(f'checksum mismatch: {ARCHIVE_NAME}\n  expected: {MODEL20_SHA256}\n  actual:   {actual}')
    extract(source, cache, extracted)
    marker.write_text(f'schema={MARKER_SCHEMA}\nurl={MODEL_URL}\nsha256={MODEL20_SHA256}\n')
    return extracted


def main(argv=None, env=None):
    """Parse the fetch options, run the fetch and print the extracted directory."""
    args = list(sys.argv[1:] if argv is None else argv)
    env = dict(os.environ if env is None else env)
    options = {'cache_dir': None, 'url': None, 'archive': None, 'force': False}
    valued = {'--cache-dir': 'cache_dir', '--url': 'url', '--archive': 'archive'}
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
