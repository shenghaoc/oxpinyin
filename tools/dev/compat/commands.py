"""Compatibility recipe dispatch; all implementations share the services beside it."""
import os
from pathlib import Path
import shlex
import sys

from ..runners import guarded
from .. import slot
from . import campaigns, fixtures, images, launcher

RECIPES = {
    'capture': fixtures.capture_fixtures,
    'drop-in': fixtures.drop_in,
    'differentials': campaigns.differentials,
    'nightly-fixtures': campaigns.nightly,
    'user-round-trip': fixtures.round_trip,
    'docker-user-round-trip': campaigns.docker_round_trip,
    'docker-round-trip-inside': campaigns.docker_inside,
    'verify-image': images.verify_image,
    'reproducible': images.reproducible,
}
NAMES = (*RECIPES, 'image-tag', 'select-runtime')


def main(argv=None):
    """Dispatch an explicit recipe, or preserve the compat launcher's option CLI."""
    args = list(sys.argv[1:] if argv is None else argv)
    def call():
        if args[:1] == ['image-tag']:
            print(images.image_tag())
            return 0
        if args[:1] == ['select-runtime']:
            rest = args[1:]
            shell = rest[:1] == ['--shell']
            if shell:
                rest.pop(0)
            lines = []
            selected = images.select_runtime(rest[0] if rest else '', output=lines.append if shell else print)
            if shell:
                for line in lines:
                    print("printf '%s\\n' " + shlex.quote(line))
                print('ctr=' + shlex.quote(selected))
            else:
                print(selected)
            return 0
        if args and args[0] in RECIPES:
            return RECIPES[args[0]](args[1:])
        try:
            return launcher.launch(args)
        except slot.Cancelled as error:
            print(str(error), file=sys.stderr)
            return 130  # compat.sh's INT, TERM and HUP trap, including cleanup
    return guarded(call)
