"""Fetch the pinned system data of the contract differential (`just fetch-contract-data`).

The data is Debian forky's `libpinyin-data` package at the one version named in
tools/oracle/libpinyin-data.json, downloaded from snapshot.debian.org by its
SHA-1 and verified by SHA-256 and by the content digest of the extracted files.
It is the system data of both `capture` (in the pin image) and `verify`; model20
is not read. It goes into contract-data/<version> under the cache root
(tools/cachedir.py). Idempotent: a verified extraction is left untouched. Its files are
read-only (the write bits cleared; on Windows the read-only attribute), a guard
against a case writing into the data through a link, not a security boundary.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import stat
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request

from tools import cachedir

ROOT = Path(__file__).resolve().parents[2]
PIN = Path(os.environ.get('CONTRACT_DATA_PIN', ROOT / 'tools/oracle/libpinyin-data.json'))
SNAPSHOT = 'https://snapshot.debian.org/file/'
ARCHITECTURES = {'x86_64': 'amd64', 'amd64': 'amd64', 'aarch64': 'arm64', 'arm64': 'arm64'}


def read_pin(path=PIN):
    return json.loads(Path(path).read_text())


def content_digest(directory):
    """The same digest contract_reference.data_identity computes."""
    rows = sorted((file.name, hashlib.sha256(file.read_bytes()).hexdigest())
                  for file in Path(directory).iterdir() if file.is_file())
    text = json.dumps(rows, sort_keys=True, separators=(',', ':'), ensure_ascii=True)
    return hashlib.sha256(text.encode('ascii')).hexdigest()


def data_identity(directory):
    """The system-data identity of a directory: digest over its files' names and SHA-256."""
    directory = Path(directory)
    if not (directory / 'table.conf').is_file():
        raise ValueError(f'{directory} is not a libpinyin system data directory (no table.conf)')
    return content_digest(directory)


def protect(directory):
    """Clear the write bits of every regular file directly under DIRECTORY.

    `os.lstat` does not follow links, so only the dataset's own files change; on
    Windows `os.chmod` without the write bits sets the read-only attribute.
    """
    for path in Path(directory).iterdir():
        st = os.lstat(path)
        if stat.S_ISREG(st.st_mode):
            os.chmod(path, stat.S_IMODE(st.st_mode) & ~(stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH))


def deb_members(payload):
    """Yield (name, bytes) for each member of an ar archive."""
    if payload[:8] != b'!<arch>\n':
        raise ValueError('not an ar archive')
    offset = 8
    while offset < len(payload):
        header = payload[offset:offset + 60]
        name, size = header[:16].decode().strip().rstrip('/'), int(header[48:58])
        offset += 60
        yield name, payload[offset:offset + size]
        offset += size + size % 2


def extract(payload, member_dir, destination):
    """Write the regular files under MEMBER_DIR of the deb's data archive into DESTINATION."""
    data = next((blob for name, blob in deb_members(payload) if name.startswith('data.tar')), None)
    if data is None:
        raise ValueError('the package has no data archive')
    destination.mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:*') as archive:
        for member in archive:
            name = member.name.removeprefix('./')
            if member.isfile() and os.path.dirname(name) == member_dir:
                (destination / os.path.basename(name)).write_bytes(archive.extractfile(member).read())


def fetch(cache, architecture=None, pin=None):
    """Return the verified data directory under CACHE, downloading the package if needed."""
    pin = pin or read_pin()
    architecture = architecture or ARCHITECTURES.get(platform.machine().lower())
    if architecture not in pin['archives']:
        raise SystemExit(f'fetch-contract-data: no package pinned for {platform.machine()}')
    entry = pin['archives'][architecture]
    final = Path(cache) / pin['version']
    with cachedir.exclusive_fetch(final):
        return _fetch_locked(final, pin, entry)


def _fetch_locked(final, pin, entry):
    destination = final / 'libpinyin/data'
    if destination.is_dir() and content_digest(destination) == pin['content_sha256']:
        protect(destination)
        return destination
    url = SNAPSHOT + entry['snapshot_sha1']
    print(f'downloading {url}', file=sys.stderr, flush=True)
    for attempt in range(3):
        try:
            with urllib.request.urlopen(url, timeout=120) as response:
                payload = response.read()
            break
        except (TimeoutError, ConnectionResetError, urllib.error.URLError) as error:
            transient = (500 <= error.code < 600 if isinstance(error, urllib.error.HTTPError)
                         else isinstance(error.reason, (TimeoutError, ConnectionResetError))
                         if isinstance(error, urllib.error.URLError) else True)
            if isinstance(error, urllib.error.HTTPError):
                error.close()  # it owns the open response; neither a retry nor the caller reads its body
            if not transient or attempt == 2:
                raise
            time.sleep(2 ** attempt)
    if hashlib.sha256(payload).hexdigest() != entry['sha256']:
        raise SystemExit('fetch-contract-data: package SHA-256 differs from tools/oracle/libpinyin-data.json')
    with tempfile.TemporaryDirectory(prefix=f'.{final.name}.', dir=final.parent) as directory:
        staging = Path(directory)
        tree = staging / 'tree'
        staged = tree / 'libpinyin/data'
        extract(payload, entry['data_dir'], staged)
        if content_digest(staged) != pin['content_sha256']:
            raise SystemExit('fetch-contract-data: extracted files differ from the pinned content digest')
        protect(staged)
        # Only the lock holder can move the damaged entry out of the way.
        if os.path.lexists(final):
            final.rename(staging / 'damaged')
        tree.rename(final)
    return destination


def default_cache(env=None):
    """The contract-data entry of the cache root.

    crates/oxpinyin-runtime/tests/common/contract_data.rs reads the same directory."""
    return cachedir.root(env) / 'contract-data'


def main(argv):
    parser = argparse.ArgumentParser(prog='just fetch-contract-data', description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--architecture', choices=sorted(set(ARCHITECTURES.values())))
    args = parser.parse_args(argv)
    try:
        print(fetch(default_cache(), args.architecture))
    except OSError as error:
        parser.exit(1, f'fetch-contract-data: {error}\n')
    return 0
