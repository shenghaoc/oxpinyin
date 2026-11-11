# k_mixture_model_to_interpolation undefined lines — normal refusal

Status: accepted class (b), 2026-10-31 UTC. Disposition: POLICY-ACCEPTED.
Pin: libpinyin 074a2219 (`utils/training/k_mixture_model_to_interpolation.cpp`
blob `957765f4`, `src/storage/tag_utility.cpp` blob `232a07dc`).

`taglib_read` uses `tokens[0]` from `split_line` as the line tag
(`tag_utility.cpp:179-190`). The reference pin dies with SIGSEGV in these cases:

- `kti-blank-line`: a section contains an empty line. `strcmp` reads the NULL
  `tokens[0]` (`:183`, `:190`).
- `kti-end-missing`: the input ends in a newline without `\end`. The final
  `std::getline` empties `linebuf`, and the body reads it again
  (`k_mixture_model_to_interpolation.cpp:86-88`). The same `strcmp` follows.
- `kti-trailing-escape`: a backslash ends a quoted token at the end of
  the line. `split_line` returns NULL (`:137`), `g_strv_length` logs GLib's
  critical (`:181`), and the read of `tokens[0]` goes through the NULL.

oxpinyin makes the pin's calls up to that read. It then writes
`k_mixture_model_to_interpolation: <reason>: <line>\n` on stderr and exits
1. Stdout and files match. `split_line`'s own critical is not written
([eio-tool-check-messages](eio-tool-check-messages.md)). The three cases in
`crates/oxpinyin-utils/tests/k_mixture_model_to_interpolation.rs` hold
oxpinyin's side.
