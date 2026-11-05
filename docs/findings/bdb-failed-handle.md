# bdb-failed-handle — Berkeley DB handles whose open failed

- **Status:** accepted · 2026-11-02 UTC · issue #1516
- **Disposition:** POLICY-ACCEPTED on the bdb cell, under the external API misuse ruling.
- **Explanation:** [compatibility register, row 113](compatibility-policy.md#bdb-failed-handles). This file holds only the digests of the accepted observations.
- **Evidence:** `just runner contract-diff bdb "$PINYIN_ORACLE_PREFIX" <libpinyin_capi.so> <libzhuyin_capi.so> -- --cases-prefix missing-system-dbm/bdb- --observations <capture>`, and `just runner tool-diff --pin-build <bdb pin tree> --subject-dir <bdb cell>/debug --observations <capture>`; then `just policy-record docs/findings/bdb-failed-handle.md <capture> --cases <IDs>`. The captures are of commit tree `54ff07ec` (code) on the host, Berkeley DB 5.3.28.

Policy evidence is in [bdb-failed-handle.json](../../fixtures/contract-policy/bdb-failed-handle.json).
