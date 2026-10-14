"""Source-reviewed J mappings, keyed by definition site at the immutable pin.

The graph retains every actual USR/full signature. Site keys apply one review
to compiler instantiations of the same reviewed template body; they do not
resolve calls or infer a counterpart from spelling. Each rule names current
Rust source anchors and explains the actual semantic correspondence.
"""

REVIEWS = {}
IDENTITY_REVIEWS = {}
# Identity reviews that exist only in one DBM cell's compiled inventory. The
# bdb ledger's synthetic-identity audit counts IDENTITY_REVIEWS, so another
# cell's identities are kept apart from it.
CELL_IDENTITY_REVIEWS = {"kc": {}, "tkrzw": {}}
# Known, tracked parity defects by pin definition site: the issue that owns
# the defect, the contract-case label prefixes deliberately narrowed around
# it (excluded from the row's shown evidence) and the reviewed reason. A row
# under such a rule is never BEHAVIOR-COVERED while the issue is open.
KNOWN_DEFECTS = {}
# Contract cases, by exact name, whose instrumented measurement depends on
# undefined behaviour in the pin: a delta ledger never credits them and
# always lists them as measurement limits, so a regeneration cannot flip on
# heap layout. Their pin/Rust verdict is kept; a DIFFER stays a DIFFER.
MEASUREMENT_LIMITS = {}


def identity_review(key, targets, reason, status="COUNTERPART", cell=None):
    """Review a compiler-generated identity without merging same-site bodies."""
    registry = IDENTITY_REVIEWS if cell is None else CELL_IDENTITY_REVIEWS[cell]
    if key in registry:
        raise ValueError(f"duplicate identity review: {key}")
    registry[key] = (status, targets, reason)

def known_defect(file, lines, issue, excluded_prefixes, reason):
    """Register a known parity defect owning the body defined at each site."""
    for line in lines:
        key = (file, line)
        if key in KNOWN_DEFECTS:
            raise ValueError(f"duplicate known defect: {key}")
        KNOWN_DEFECTS[key] = dict(issue=issue, excluded=tuple(excluded_prefixes), reason=reason)


def measurement_limit(names, reason):
    """Register contract cases whose measurement is known to depend on pin UB."""
    for name in names:
        if name in MEASUREMENT_LIMITS:
            raise ValueError(f"duplicate measurement limit: {name}")
        MEASUREMENT_LIMITS[name] = reason


def review(file, lines, targets, reason, status="COUNTERPART"):
    for line in lines:
        key = (file, line)
        if key in REVIEWS:
            raise ValueError(f"duplicate review: {key}")
        REVIEWS[key] = (status, targets, reason)

C = "crates/oxpinyin-data/src/chewing_table.rs"

E = "crates/oxpinyin-data/src/table_entries.rs"

R = "crates/oxpinyin-data/src/row_format.rs"

U = "crates/oxpinyin-user/src/store.rs"

P = "crates/oxpinyin-user/src/persistence.rs"

D = "crates/oxpinyin-data/src/dict.rs"

# Source-file review batches are separate commit-sized units.
import importlib
from pathlib import Path


def load_rules():
    for batch in sorted(Path(__file__).with_name("coverage_review_rules").glob("*.py")):
        importlib.import_module("coverage_review_rules." + batch.stem)


def reload_rules():
    """Re-read every review batch into the same registries, in place.

    A delta ledger classifies only after hours of measurement; a review
    batch written meanwhile is read at classification time, as the bdb
    renderer re-reads its citation rule. Registries keep their identity so
    importers that bound them by name observe the refreshed rules.
    """
    import sys
    for registry in (REVIEWS, IDENTITY_REVIEWS, KNOWN_DEFECTS, MEASUREMENT_LIMITS, *CELL_IDENTITY_REVIEWS.values()):
        registry.clear()
    for batch in sorted(Path(__file__).with_name("coverage_review_rules").glob("*.py")):
        name = "coverage_review_rules." + batch.stem
        if name in sys.modules:
            importlib.reload(sys.modules[name])  # re-executes the batch once
        else:
            importlib.import_module(name)  # a batch written after start-up: executed once


load_rules()
