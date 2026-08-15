---
inclusion: always
---

# Agent protocol

Follow `/AGENTS.md` (wins on conflict). Tiers: `[A]` architect · `[B]` basic ·
`[H]` human — never hand `[A]`/`[H]` work to a basic agent.

Implement from frozen SPECs/fixtures only. Assisted-by:

    Assisted-by: Kiro:kiro-<id>

Nothing after the model. The model token must contain an ASCII letter
(`Kiro:kiro-1` passes; `Kiro:4.6` fails). Set semantics: `/AGENTS.md`.
