## Summary

<!--
Write one short paragraph. State the problem, the reason for the change,
and the result.
-->

## Changes

<!--
Use a few bullets for substantive changes. Do not repeat the file list.
If you add a new driver, state why no existing driver could take the case
(docs/runbooks/verification.md, rule 6).
-->

## Results

<!--
Run the local checks in [verification rule 5](https://github.com/shenghaoc/oxpinyin/blob/main/docs/runbooks/verification.md#5-local-test-gate); they scale with the change and its stage.

State the results and the command that reproduces them.
CI on the PR head is the full gate (verification rule 5); state the local checks you ran before the push.
Disclose any required check that failed or could not run as a limitation.
Omit checks that do not apply.
Follow docs/runbooks/verification.md#7-proportionate-verification: no raw logs, captures,
evidence archives, upload requests or separate findings document by default.
-->

## Compatibility and impact

<!--
State the effects on the C ABI, pinned behaviour, persistent data,
performance, and memory.
For a change that alters an algorithm, a data structure, allocation behaviour
or an input bound, state the source-derived complexity argument
(Source policy, controlling rule).
When a measurement is required, report the question, the arms and inputs
relevant to it, the decision rule and the result, with the command needed to
reproduce the comparison. One command may cover the whole table; include
inputs/build/environment details that matter.
For changes affecting shipping libraries, copy the base and PR sizes from the
`shipping-sizes` job summary and note increases over base (Constitution 2). Do not
build release libraries locally for them. Docs-only work needs no size build.
Justify each time/space trade-off (Source policy).
See docs/packaging.md#function-layout for the function-layout ruling.
Write "None" when nothing applies.
-->

## Related work

<!--
Link related issues. Use "Fixes #N" when this PR fixes an issue.
For a stacked PR, name its direct parent PR.
-->
