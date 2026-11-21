"""Shared diagnostics and failure handling for native tools."""
from collections.abc import Callable
import sys

from . import pool, slot
from .cell import CellError


class Finished(Exception):
    def __init__(self, status: int):
        self.status = status


def emit(value: str = '', *, error: bool = False) -> None:
    print(value, file=sys.stderr if error else sys.stdout, flush=True)


def write(data: bytes, *, error: bool = False) -> None:
    stream = sys.stderr if error else sys.stdout
    stream.flush()
    stream.buffer.write(data)
    stream.flush()


def status(code: int) -> int:
    return 128 - code if code < 0 else code


def guarded(call: Callable[[], int]) -> int:
    """Run CALL under the cancellation handlers and return its status.

    The failures every runner shares end in the same statuses and messages: a
    finished or refused run, a cancellation, a missing
    command (127), and any other OS or value error (1).
    """
    try:
        with slot.cancellation_handlers():
            return call()
    except Finished as error:
        return error.status
    except CellError as error:
        emit(str(error), error=True)
        return error.status
    except (pool.UnitStopped, pool.UnitsCancelled, slot.Cancelled) as error:
        emit(str(error), error=True)
        return slot.cancellation_status()
    except FileNotFoundError as error:
        emit(str(error), error=True)
        return 127
    except (OSError, ValueError, RuntimeError) as error:
        emit(f'FAIL: {error}', error=True)
        return 1
