"""Typed records shared by host reads, heavy admission and reservations.

The records keep their existing dictionary and JSON field names. These
annotations describe their shapes for readers; they do not validate or
rewrite a record at runtime. All memory quantities are whole mebibytes.
`None` means the quantity is unknown, never that it is zero: a known zero is
the integer `0`.
"""
from typing import Literal, NotRequired, TypedDict

# A whole number of mebibytes. Fields typed `MiB | None` keep unknown as None
# and represent a known zero as 0.
MiB = int

ContainerKind = Literal['docker', 'podman', 'container']
ContainerPhase = Literal['creating', 'created', 'create-failed']


class ResourceSnapshot(TypedDict):
    """One host resource read, before any reservation is subtracted.

    The memory fields are whole MiB: available_mib is the smallest known
    memory room, memory_limit_mib the smallest known hard limit, and total_mib
    the detected physical total. Each is None when unreadable, and 0 is a
    known zero. cpus and cargo_cpus are whole CPU counts.
    """

    available_mib: MiB | None
    memory_limit_mib: MiB | None
    total_mib: MiB | None
    cpus: int
    cargo_cpus: int
    notes: list[str]


class HeadroomResources(TypedDict):
    """The resource fields the desktop-headroom decision reads, in whole MiB.

    None means unknown; 0 is a known zero.
    """

    memory_limit_mib: MiB | None
    total_mib: MiB | None


class ReservationTotals(TypedDict):
    """MiB charged to live heavy slots and compatibility containers.

    slot_reserved_mib is always a known total. container_reserved_mib is None
    when an invalid container record makes the container budget unknown.
    """

    slot_reserved_mib: MiB
    container_reserved_mib: MiB | None


class BudgetedResources(ResourceSnapshot, ReservationTotals):
    """A resource read after live reservations are subtracted.

    available_mib is already net of both reservation totals. The individual
    totals stay beside it so a refusal can report what the decision subtracted.
    """


class HeavyHolder(TypedDict):
    """The advisory JSON record in a heavy-slot file.

    A provisional record omits estimate_mib. When present, estimate_mib is a
    whole-MiB estimate; 0 is a known zero, not an unknown value. Readers
    ignore a non-positive or non-integer estimate.
    """

    pid: int
    worktree: str
    command: str
    since: str
    token: str
    estimate_mib: NotRequired[MiB]


class ContainerRef(TypedDict):
    """The runtime identity needed to inspect or list one reserved container."""

    runtime: str
    kind: ContainerKind
    name: str
    token: str


class ContainerReservationRecord(ContainerRef):
    """The JSON reservation record for one compatibility container.

    memory_mib is the container's full hard limit, in MiB, and is positive:
    the launcher reserves the whole limit, not a residual. A zero or unknown
    (None) cap is invalid. schema is the serialized record version.
    """

    schema: Literal[1]
    memory_mib: MiB
    phase: ContainerPhase
    since: str
