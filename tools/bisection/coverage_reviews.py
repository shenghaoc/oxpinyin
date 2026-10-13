"""Source-reviewed J mappings, keyed by definition site at the immutable pin.

The graph retains every actual USR/full signature. Site keys apply one review
to compiler instantiations of the same reviewed template body; they do not
resolve calls or infer a counterpart from spelling. Each rule names current
Rust source anchors and explains the actual semantic correspondence.
"""

REVIEWS = {}
IDENTITY_REVIEWS = {}


def identity_review(key, targets, reason, status="COUNTERPART"):
    """Review a compiler-generated identity without merging same-site bodies."""
    if key in IDENTITY_REVIEWS:
        raise ValueError(f"duplicate identity review: {key}")
    IDENTITY_REVIEWS[key] = (status, targets, reason)

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

for batch in sorted(Path(__file__).with_name("coverage_review_rules").glob("*.py")):
    importlib.import_module("coverage_review_rules." + batch.stem)
