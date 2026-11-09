# k_mixture_model_to_interpolation undefined lines — normal refusal

Status: accepted class (b), 2026-10-31 UTC. Disposition: POLICY-ACCEPTED.
Pin: libpinyin 074a2219 (`utils/training/k_mixture_model_to_interpolation.cpp`
blob `957765f4`, `src/storage/tag_utility.cpp` blob `232a07dc`).

`taglib_read` takes `tokens[0]` from `split_line` as the line tag
(`tag_utility.cpp:179-190`). The reference pin dies with SIGSEGV in:

- `kti-blank-line`: an empty line in a section; `strcmp` reads the NULL
  `tokens[0]` (`:183`, `:190`).
- `kti-end-missing`: input ending in a newline without `\end`; the final
  `std::getline` empties `linebuf` and the body re-reads it
  (`k_mixture_model_to_interpolation.cpp:86-88`), the same `strcmp`.
- `kti-trailing-escape`: a backslash ending a quoted token at the end of
  the line; `split_line` answers NULL (`:137`), `g_strv_length` logs GLib's
  critical (`:181`), and `tokens[0]` reads through the NULL.

oxpinyin makes the pin's calls up to that read, then writes
`k_mixture_model_to_interpolation: <reason>: <line>\n` on stderr and exits
1. Stdout and files match. `split_line`'s own critical is not written
([eio-tool-check-messages](eio-tool-check-messages.md)). The three cases in
`crates/oxpinyin-utils/tests/k_mixture_model_to_interpolation.rs` hold
oxpinyin's side.
