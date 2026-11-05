# KMM tool asserts and their policy records

Date: 2026-10-31 UTC · Status: **policy record** · Branch: `wip/eio5-kmm`.

The seven k-mixture-model utilities at libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99` reproduce the pin's abort sites
with `std::process::abort()`. The reference build keeps `assert` and
`check_result` active (`utils/training/*_k_mixture_model.cpp`, no `NDEBUG`),
so the pin prints its macro text on stderr and raises `SIGABRT`. Under the
2026-10-31 UTC maintainer ruling in
[eio-tool-check-messages](eio-tool-check-messages.md), oxpinyin does not
imitate the macros' text; stdout, the signal and the file inventory match.

Three registered `tool-diff` cases reach such a site:

- `kmm-merge-no-result-file` — no `--result-file`, so the target is the
  pin's NULL attach and `merge_magic_header`'s
  `check_result(target->set_magic_header(...))` fails
  (`merge_k_mixture_model.cpp:127`).
- `kmm-estimate-zero-total` — the candidate's magic header total frequency
  is zero, and `compute_interpolation` asserts
  `0 != magic_header.m_total_freq` (`estimate_k_mixture_model.cpp:45`).
- `kmm-gen-good-document` — no `--k-mixture-model-file`, so the first
  document trains into a NULL handle and `main`'s
  `check_result(bigram.get_magic_header(...))` fails
  (`gen_k_mixture_model.cpp:380`).

Each case registers `pin_signal: -6` and `reproduces_signal: true`, and
`fixtures/contract-policy/eio-kmm-tools.json` binds the complete observations.
These utilities open the working container, so the entries are per cell, unlike the database-free
marker records.

Refreshed 2026-11-03 UTC: the entries were first bound at `494766c3`,
when each cell's `fixtures/w3` directory still held `datagen-manifest.txt`.
`4d60c774` (#1500) deleted that file. The tool runner copies the whole
cell directory into the case's working directory, so both sides' `files`
listing lost one name. That is the only change: exit, stdout and stderr
digests are as first recorded.

Policy evidence is in [eio-kmm-tools.json](../../fixtures/contract-policy/eio-kmm-tools.json).
