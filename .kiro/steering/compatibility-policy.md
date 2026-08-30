---
inclusion: always
---
# Compatibility policy (compact)

Canonical text: `docs/findings/compatibility-policy.md` (policy,
2026-08-28). The goal is a drop-in replacement: rename the built object to
`libpinyin.so.15`, put it on the library path, and unmodified consumers work
against the data already on the system.

**E2E I/O rule.** For every exported symbol in the consumer union (58
symbols), given the same inputs and state, oxpinyin MUST return
byte-identical output to the pinned libpinyin 2.11.91 — return status,
out-parameters and the data they point to, written lengths, and any state
transition on the handle. Divergence is permitted only under the four
classes below; anything else is a defect to revert, not a divergence to
record.

**(a) MATH** — platform-dependent floating-point accumulation: a
transcendental in the accumulation, not merely a float in the call graph.
The sentence-surface residual lives here, recommended to freeze as
permanent; the freeze is the maintainer's call.

**(b) MEMORY SAFETY** — upstream is UB and Rust structurally prevents it;
covers only cases where reproduction is structurally impossible.

**(c) AVAILABILITY** — upstream asserts or aborts on input a caller can
supply; oxpinyin returns `false`/`Err` and logs the point. Covers aborts,
not wrong-but-defined answers.

**(d) CONSUMER SCOPE** — only what ibus-libpinyin 1.16.5 and
fcitx-libpinyin actually call; dead code is not a call site.

A stub returning `false` is not compliance — it is a defect. Probe coverage
is itself a deliverable: a consumer-union symbol with no differential probe
is unverified, not compliant.
