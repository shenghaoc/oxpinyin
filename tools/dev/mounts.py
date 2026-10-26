"""Mount-type reader shared by teardown.py and test_runner.py. Standard library only."""
from pathlib import Path
import re


def filesystem(path, mountinfo=Path('/proc/self/mountinfo')):
    """Read the actual mount type; ext2/3/4 share statfs's magic number.

    Raise ValueError when no mount covers path, OSError when mountinfo cannot be read.
    """
    path = Path(path).resolve()
    matches = []
    for line in Path(mountinfo).read_text().splitlines():
        fields, separator, trailer = line.partition(' - ')
        if not separator:
            continue
        mount = re.sub(r'\\([0-7]{3})', lambda match: chr(int(match[1], 8)), fields.split()[4])
        if path.is_relative_to(mount):
            matches.append((len(mount), trailer.split()[0]))
    if not matches:
        raise ValueError('filesystem type is unknown')
    return max(matches)[1]
