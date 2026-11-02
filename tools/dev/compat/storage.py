"""Runtime-neutral compatibility storage ownership and in-volume cleanup."""
import hashlib
from pathlib import Path
import re

from .process import ROOT

VOLUME = re.compile(r'oxpinyin-compat-(?:v2-)?[0-9a-f]{16}\Z')
LEGACY = re.compile(r'oxpinyin-compat-[0-9a-f]{16}\Z')
PROJECT = 'io.oxpinyin.project'
OWNER = 'io.oxpinyin.owner'
PURPOSE = 'io.oxpinyin.purpose'
PURPOSE_VALUE = 'checkout'


def volume_name(root=ROOT):
    """Derive the compatibility volume from the checkout root."""
    return 'oxpinyin-compat-v2-' + owner_id(root)


def owner_id(root=ROOT):
    """Identify a checkout without embedding its host path in volume metadata."""
    return hashlib.sha256(str(Path(root)).encode()).hexdigest()[:16]


def labels(root=ROOT):
    return {PROJECT: 'oxpinyin', OWNER: owner_id(root), PURPOSE: PURPOSE_VALUE}


def classify(name, metadata, owners):
    """Classify only labels matching both the naming and ownership contract."""
    labels = metadata.get('labels') or {}
    if LEGACY.fullmatch(name) and not labels:
        return 'legacy, unlabelled'
    owner = labels.get(OWNER, '')
    expected = 'oxpinyin-compat-v2-' + owner
    if (name != expected or not re.fullmatch('[0-9a-f]{16}', owner)
            or labels.get(PROJECT) != 'oxpinyin' or labels.get(PURPOSE) != PURPOSE_VALUE):
        return 'unowned or invalid labels'
    return 'active owner ' + owner if owner in owners else 'orphaned owner ' + owner


def volume_names(output):
    """Recognize only exact project volume names from a runtime listing."""
    return sorted({line.split()[0] for line in output.splitlines()
                   if line.split() and VOLUME.fullmatch(line.split()[0])})


def main(argv):
    """Route lifecycle operations through the existing launcher adapter."""
    from . import launcher
    return launcher.storage_main(argv)

# Executed inside the maintenance container; its sole writable mount is /state.
# The lock matches the compatibility guest's lock and is held across deletion.
CLEAN_GUEST = r'''import fcntl, os, pathlib, re, shutil, subprocess, sys
root = pathlib.Path(sys.argv[1])
requested = set(sys.argv[2:])
allowed = {'targets', 'work', 'results'}
if not requested or not requested <= allowed:
    raise SystemExit('invalid cleanup selection')
if root.is_symlink() or not root.is_dir():
    raise SystemExit('invalid state root')
lock = root / 'run.lock'
fd = os.open(lock, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
with os.fdopen(fd, 'r+') as stream:
    try:
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise SystemExit('compatibility volume is in use')
    paths = []
    def add(path):
        if path.is_symlink():
            raise SystemExit('refusing redirected cleanup path: ' + str(path))
        if path.exists():
            if not path.is_dir():
                raise SystemExit('refusing non-directory cleanup path: ' + str(path))
            paths.append(path)
    if 'work' in requested:
        add(root / 'work')
    if 'results' in requested:
        results = root / 'results'
        if results.is_symlink():
            raise SystemExit('refusing redirected results directory')
        add(results / 'container')
    if 'targets' in requested:
        add(root / 'targets')
        container = root / 'container'
        if container.is_symlink():
            raise SystemExit('refusing redirected container directory')
        if container.exists():
            if not container.is_dir():
                raise SystemExit('invalid container directory')
            for image in container.iterdir():
                if image.is_symlink() or not image.is_dir() or not re.fullmatch('[0-9a-f]{16}', image.name):
                    raise SystemExit('refusing unowned image namespace: ' + str(image))
                add(image / 'targets')
    for path in paths:
        subprocess.run(['du', '-sk', str(path)], check=True)
    sys.stdout.flush()
    for path in paths:
        shutil.rmtree(path)
    print('OXPINYIN_CLEAN_READY', flush=True)
    sys.stdin.readline()
'''
