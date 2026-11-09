# Runbooks

Step-by-step procedures for the things this repository does by hand.
Each page says what it needs, what to run, and what "done" looks like;
the reasoning behind each procedure lives in the findings it links.

| Runbook | When |
| --- | --- |
| [`setup.md`](setup.md) | setting up native development on Linux, macOS or Windows, and the contribution checks |
| [`verification.md`](verification.md) | selecting the cells, local checks and CI merge conditions for a change |
| [`backends.md`](backends.md) | building and testing under a store backend other than the default |
| [`oracle.md`](oracle.md) | building the pin-built libpinyin and running the differentials against it |
| [`contract-references.md`](contract-references.md) | the C-boundary tests' frozen expectations, the pin-bump rule and the local cross-open check |
| [`pin-captures.md`](pin-captures.md) | staging the pin's observations of the apostrophe inputs for review |
| [`cloud.md`](cloud.md) | provisioning a remote Linux host (container runtime, oracle, data) so every gate runs there |
| [`benches.md`](benches.md) | running a Callgrind or Massif recipe, or a plain timing |
| [`glib-call-sites.md`](glib-call-sites.md) | regenerating the GLib and libc call-site list from the pin |
| [`goldens-and-pins.md`](goldens-and-pins.md) | refreshing a committed golden, moving the oracle pin, re-freezing a pinned number |
| [`release.md`](release.md) | cutting a release and what the packaging pipeline gates |
| [`tool-diff.md`](tool-diff.md) | running and extending the frozen compatibility cases of the pin's standalone utilities |

Read the [contributor procedures](../../CONTRIBUTING.md) for approval conditions, frozen SPECs and rebases.
The [compatibility policy](../findings/compatibility-policy.md) defines permitted divergences.
