# bdb-failed-handle — Berkeley DB handles whose open failed

- **Status:** accepted · 2026-11-02 UTC · issue #1516
- **Disposition:** POLICY-ACCEPTED on the bdb cell, under the external API misuse ruling.
- **Explanation:** [compatibility register, row 113](compatibility-policy.md#bdb-failed-handles) gives the pin derivation and oxpinyin's handle discipline. This file lists the accepted cases.
- **Difference:** every answer, stdout, exit status and file is the pin's. The only difference is on stderr: oxpinyin does not write Berkeley DB's `BDB1565 DB-><method>: method not permitted before handle's open method` lines. Observed at code tree `54ff07ec` with Berkeley DB 5.3.28.
- **Contract cases (29):** `missing-system-dbm/bdb-<facade>-<table>-<scenario>` for
  - pinyin: `addon-phrase`, `addon-pinyin`, `bigram`, `phrase`, `pinyin` and `punctuation`, each with `lifecycle`, `operations` and `overlay`, plus `pinyin-unloaded`;
  - zhuyin: `bigram`, `phrase` and `pinyin`, each with `lifecycle`, `operations` and `overlay`, plus `pinyin-unloaded`.
- **Utility cases (19):** `gen_ngram-{empty-stdin,missing-bigram-file,ordinary,skip-pi-gram,too-many-arguments}`, `gen_deleted_ngram-{empty-stdin,missing-bigram-file,ordinary,too-many-arguments}`, `estimate_interpolation-{missing-bigram-file,missing-deleted-bigram}`, `eval_correction_rate-missing-bigram-file`, `exp-missing-bigram`, `kmm-estimate-{existing-empty,no-models}`, `kmm-export-{absent-file,existing-empty}` and `kmm-validate-{existing-empty,missing-file}`. The three `kmm-*-existing-empty` cases joined on 2026-11-03 UTC (`0d81b907`). The frozen utility cases in `crates/oxpinyin-utils/tests/` hold oxpinyin's side and name this record.
