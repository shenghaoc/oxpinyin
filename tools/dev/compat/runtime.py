"""Runtime adapter selection and runtime-owned image and volume metadata."""
import json
import os
from pathlib import Path
import re
import shutil
import sys

from .process import capture, fail, text


def content_digest(runtime, kind, image):
    """Read the full local image content identity, rejecting ambiguous metadata."""
    done = capture([runtime, 'image', 'inspect', image])
    try:
        rows = json.loads(done.stdout)
        if not isinstance(rows, list) or len(rows) != 1:
            raise ValueError('expected one image')
        value = rows[0]['id'] if kind == 'container' else rows[0]['Id']
        value = value.removeprefix('sha256:')
        if not re.fullmatch('[0-9a-f]{64}', value):
            raise ValueError('invalid image digest')
        return value
    except (KeyError, TypeError, ValueError) as error:
        raise ValueError(f'cannot establish image content identity: {error}') from error


def verify_created_image(record, expected):
    """Check the created container's image before starting its payload."""
    from .. import host
    row = host.inspect_container(record)
    try:
        value = (row['configuration']['image']['descriptor']['digest']
                 if record['kind'] == 'container' else row['Image'])
        if value.removeprefix('sha256:') != expected:
            raise ValueError('created container image digest differs from inspected image')
    except (KeyError, TypeError, AttributeError) as error:
        raise ValueError('created container image identity unavailable') from error
def volume_metadata(runtime, kind, name):
    """Return validated volume labels, or None if the runtime reports no volume."""
    done = capture([runtime, 'volume', 'inspect', name], check=False, stderr=False)
    if done.status:
        diagnostic = done.stderr.decode(errors='replace')
        if re.search(r'not found|no such volume|does not exist', diagnostic, re.I):
            return None
        raise ValueError(f'cannot inspect volume {name}: {diagnostic.strip()}')
    try:
        rows = json.loads(done.stdout)
        if not isinstance(rows, list) or len(rows) != 1:
            raise ValueError('expected exactly one volume')
        if not isinstance(rows[0], dict):
            raise ValueError('expected an object')
        row = rows[0].get('configuration', {}) if kind == 'container' else rows[0]
        if not isinstance(row, dict):
            raise ValueError('expected volume configuration')
        found = row.get('name') if kind == 'container' else row.get('Name')
        labels = (row.get('labels') if kind == 'container' else row.get('Labels')) or {}
        if found != name or not isinstance(labels, dict):
            raise ValueError('volume identity or labels mismatch')
        return {'labels': labels}
    except (TypeError, KeyError, ValueError) as error:
        raise ValueError(f'invalid volume inspection for {name}: {error}') from error


def select_runtime(override='', *, env=None, host=None, output=print):
    """Preserve platform runtime order, explicit override, and unverified refusals."""
    environment = os.environ if env is None else env
    host = host or text(['uname', '-s'], env=environment)
    if host == 'Darwin':
        candidates = ['container', 'podman', 'docker']
    elif host.startswith(('MINGW', 'MSYS', 'CYGWIN')) or environment.get('WSL_DISTRO_NAME'):
        candidates = ['wslc', 'podman', 'docker']
    else:
        candidates = ['podman', 'docker']
    override = override or environment.get('OXPINYIN_CONTAINER', '')
    selected = override or next((name for name in candidates if shutil.which(name, path=environment.get('PATH'))), '')
    if not selected or not shutil.which(selected, path=environment.get('PATH')):
        fail(f'FAIL: no runtime available; candidates in order: {" ".join(candidates)} (override: {override or "none"})')
    if Path(selected).name == 'wslc':
        fail(f'Runtime: {selected}; version unavailable (adapter not yet verified)\n'
             'FAIL: wslc adapter not yet verified; select an exercised runtime explicitly with OXPINYIN_CONTAINER or --runtime')
    done = capture([selected, '--version'], env=environment, check=False)
    if done.status:
        fail(f'FAIL: cannot query runtime version: {selected}')
    output(f'Runtime: {selected}; {done.stdout.decode().rstrip(chr(10))}')
    if Path(selected).name == 'container':
        print('Apple container interoperability and native-platform execution are not yet verified by the compat smoke.', file=sys.stderr)
    return selected
