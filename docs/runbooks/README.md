# Runbooks

These pages are the step-by-step procedures for the manual tasks in this repository.
Each page states what it needs, what to run and what "done" looks like.
The findings that each page links give the reasons for the procedure.

| Runbook | Use it to |
| --- | --- |
| [`setup.md`](setup.md) | set up native development on Linux, macOS or Windows, and run the contribution checks |
| [`verification.md`](verification.md) | select the cells, local checks and CI merge conditions for a change |
| [`backends.md`](backends.md) | build and test under a store backend other than the default |
| [`oracle.md`](oracle.md) | build the pin-built libpinyin and run the differentials against it |
| [`contract-references.md`](contract-references.md) | find the frozen expectations of the C-boundary tests, the pin-bump rule and the local cross-open check |
| [`pin-captures.md`](pin-captures.md) | stage the pin's observations of the apostrophe inputs for review |
| [`cloud.md`](cloud.md) | provision a remote Linux host (container runtime, oracle, data) so that every gate runs there |
| [`benches.md`](benches.md) | run a Callgrind or Massif recipe, or a plain timing |
| [`glib-call-sites.md`](glib-call-sites.md) | regenerate the GLib and libc call-site list from the pin |
| [`goldens-and-pins.md`](goldens-and-pins.md) | refresh a committed golden, move the oracle pin, or re-freeze a pinned number |
| [`release.md`](release.md) | cut a release, and see what the packaging pipeline gates |
| [`tool-diff.md`](tool-diff.md) | run and extend the frozen compatibility cases of the pin's standalone utilities |

The [contributor procedures](../../CONTRIBUTING.md) give the approval conditions, frozen SPECs and rebases.
The [compatibility policy](../findings/compatibility-policy.md) defines the permitted divergences.
