# Contributing

oxpinyin reproduces a pinned libpinyin: its behaviour follows the pin, defects included ([source policy](docs/source-policy.md), [compatibility policy](docs/findings/compatibility-policy.md)).

## Workflow

1. Set up native development with the [setup guide](docs/runbooks/setup.md) when the checks for the change need it.
2. Find the crate that owns the behaviour in the [architecture](docs/architecture.md); the [Rust conventions](docs/rust-conventions.md) apply to Rust code.
3. Make the change on a branch. The [verification procedure](docs/runbooks/verification.md#verification-scope-and-precedence) selects the checks for every change, documentation included; the [testing overview](docs/testing/README.md#where-tests-belong) describes where a test belongs.
4. Open a pull request with the [PR template](.github/PULL_REQUEST_TEMPLATE.md), stating the result and the command that reproduces it. A stacked PR describes only its own diff and names its direct parent PR. An issue follows a search of open and closed issues, covers one outcome unless it tracks others, and uses an [issue template](.github/ISSUE_TEMPLATE/) and the existing labels.
5. A PR merges after at least one bot review.

## Approval conditions

These changes need the maintainer's approval first: a change to the C interface or ABI; a new or upgraded dependency; a change to a golden or to CI policy; and a new or changed compatibility decision, including a product-behaviour change that the compatibility policy's [decision order](docs/findings/compatibility-policy.md#decision-order) does not class as a language artefact.

No document is frozen: a SPEC states the pin's behaviour. A SPEC or a contract that disagrees with the pin's source, where the code agrees with the pin, is corrected without approval.

`README.md` and `README.中文.md` stay consistent: a change to the content of one changes both in the same PR. A Chinese wording correction that does not change content needs no English change.

Task procedures: [backends](docs/runbooks/backends.md), [oracle](docs/runbooks/oracle.md), [remote Linux host](docs/runbooks/cloud.md), [measurement](docs/runbooks/benches.md), [goldens and pins](docs/runbooks/goldens-and-pins.md), [packaging](docs/packaging.md) and [release](docs/runbooks/release.md).
