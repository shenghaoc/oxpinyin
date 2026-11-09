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

In each case both sides raise SIGABRT with the same stdout and files; only
the pin writes the macro text on stderr. These utilities open the working
container, so each case is accepted on bdb, kc and tkrzw, unlike the
database-free records. The cases in `crates/oxpinyin-utils/tests/kmm_tools.rs`
hold oxpinyin's side.
