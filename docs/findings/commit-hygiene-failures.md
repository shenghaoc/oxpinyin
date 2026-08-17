# Commit-hygiene failures

Date: 2026-08-17 · Status: closed historical record. History is not
rewritten.

## Purpose

`main` carries trailer and status-check violations. The cited SHAs in
findings, the roadmap, PR bodies, and this file must stay valid, so the
history stands as-is. The failures are owned here rather than cleaned by
a rewrite.

A pre-release history rewrite would have made every SHA cited before
the rewrite a dangling pointer. That cost is larger than the cost of
leaving a documented mess in the early history. `v0.1.0` tags on the
existing history; it does not wait for a rewrite that will not happen.

This file is the catalogue. It does not change any existing commit.

## Audit

Every commit reachable from `origin/main` at the audit tip
`12af69521c00f08328e3cf97b3cf1164a74d4d45` (181 commits, linear, no
merges; first commit `9ab6428` 2026-08-08). Trailers were parsed, not
grepped:

```sh
git rev-list --count origin/main
git log --reverse \
  --format='%H %h %ad %s%n%(trailers:only,unfold)%n---' \
  --date=short origin/main
git log --reverse --format='%H%n%B%n---' origin/main
```

`%(trailers:only,unfold)` is the same parser the commit-message linter
uses. The full body scan was a second pass for `AI-session:` /
`Assisted-by:` lines that failed to parse as trailers; it added nothing.

Other trailer keys (`Signed-off-by`, `Co-authored-by`) and
author/committer identities were scanned so a fourth class would not
hide inside the three named ones. They are not catalogued here.

## 1. `AI-session:` trailers

### The rule as it should have been

Do not emit an `AI-session:` trailer. Attribution is `Assisted-by:`
only. The on-tree linter's R3 does something weaker: if an
`AI-session:` trailer is present, its value must be exactly `true` and
an `Assisted-by:` trailer must then exist; any other value fails. R3
does not reject the trailer's presence.

### What went wrong

Eight commits on `main` carry `AI-session: true`. All eight also carry
a well-formed `Assisted-by:` trailer, so today's R3 check accepts every
one of them. The trailer was emitted after the linter landed on `main`
(`66dfe06`, 2026-08-15).

### Violating commits

| short | date | subject | offending line |
|---|---|---|---|
| `3ed79b1` | 2026-08-16 | fix(core): reconstruct fewest_keys from the filtered final_step | `AI-session: true` |
| `8ac0bdf` | 2026-08-16 | feat(capi): close W8 fork-bootstrap 51-symbol and C++ header gaps | `AI-session: true` |
| `3a49766` | 2026-08-16 | fix(capi): store parse length from the session fewest-keys walk | `AI-session: true` |
| `0d1e5f2` | 2026-08-16 | fix(capi): remask PINYIN_INCOMPLETE on live instances | `AI-session: true` |
| `3d55553` | 2026-08-16 | feat(capi): load real interpolation2 unigrams or fail loudly | `AI-session: true` |
| `3ec6172` | 2026-08-16 | refactor: stack-review cleanups that do not change merge-time behavior | `AI-session: true` |
| `20e6b3a` | 2026-08-17 | feat(w13): add double-pinyin and standard bopomofo schemes | `AI-session: true` |
| `5fcce53` | 2026-08-17 | fix(core): stop zhuyin parse on illegal tones | `AI-session: true` |

No other `AI-session:` value occurs. None of the eight is body-only;
every line parsed as a trailer.

### Enforcement now in place

[PR #95](https://github.com/shenghaoc/oxpinyin/pull/95) (open as of this
audit; one file, `AGENTS.md`) rewords R3 to "Do not emit an
`AI-session:` trailer" and states there is no change to linter
behaviour. It does not touch `.github/scripts/lint-commits.sh`.

**#95's lint would not catch this class today.** #95 introduces no
lint. The on-tree R3 check — landed on `main` in `66dfe06` (2026-08-15),
authored in [PR #56](https://github.com/shenghaoc/oxpinyin/pull/56) —
accepts `AI-session: true` plus a valid `Assisted-by:`. That is every
row above.

Prevention of this class is the `AGENTS.md` prohibition (once #95
lands), not a machine gate.

## 2. `Assisted-by` values

### The rule as it should have been

`Assisted-by: <AgentName>:<model-id>`, nothing after the model.
Trailers are a set. The linter's R2 mechanizes four conditions when
the trailer is present: house shape, the model token contains at least
one ASCII letter (`Grok:4.6` fails, `Grok:grok-4.6` passes), no
placeholder text, no duplicate identical lines. Semantics beyond shape
stay a human attestation: the linter has no agent/model allowlist.

Absence of the trailer is not a lint failure unless R3 has promoted it
(`AI-session: true`). Commits with no `Assisted-by:` cannot be
classified as "human" or "AI-assisted" from the message alone.

### What went wrong

180 of 181 commits carry at least one `Assisted-by:` trailer (222
trailer lines, 19 distinct values). Fifteen of those commits fail R2
condition 2: the model token is a bare version number. Twenty-four
commits carry more than one distinct `Assisted-by:` line; R2 allows
that (set semantics). No commit uses a wrong key casing, a missing
colon, an empty half, a placeholder, or a duplicate identical line.

#95 introduced no allowlist. Values that pass R2 are listed for the
maintainer to mark; this audit does not call a model-id "wrong"
beyond the mechanical checks.

### Distinct values

<!-- maintainer: mark invalid model ids -->

| value | n | mechanical flag |
|---|---|---|
| `Grok:grok-4.6` | 32 | — |
| `Claude:claude-opus-5` | 29 | — |
| `Copilot:deepseek-v4-pro` | 23 | — |
| `Kiro:GPT-5.6-Sol` | 22 | — |
| `Claude:claude-opus-4-8` | 17 | — |
| `Claude:deepseek-v4-pro` | 14 | — |
| `Grok:4.5` | 14 | R2 condition 2: model token has no ASCII letter |
| `Grok:grok-4.5` | 14 | — |
| `Claude:claude-opus-4-6` | 13 | — |
| `Kiro:Claude-Opus-5` | 13 | — |
| `Claude:claude-fable-5` | 10 | — |
| `MuseCode:muse-spark` | 7 | — |
| `DeepSeek:deepseek-v4-pro` | 5 | — |
| `Copilot:muse-spark-1.2-contributor` | 3 | — |
| `opencode:deepseek-v4-pro` | 2 | — |
| `Codex:deepseek-v4-pro` | 1 | — |
| `Codex:gpt-5` | 1 | — |
| `Grok:4.6` | 1 | R2 condition 2: model token has no ASCII letter |
| `opencode:muse-spark-1.2-contributor` | 1 | — |

Full hashes per value:

`Grok:grok-4.6` (32):
`92fc44bff8424fa59a9417cddd19e69343a37343`
`fcee0783f1108b9f7bf9c10e680cbddd8d585be7`
`e7dabed19b77273de232d4f2c100b025f5e3222d`
`730d46d4665f6e66f704144593812b87b03bb1f8`
`10bee2896381fd1a5091b4cacb985e4623605b74`
`258fe1b0cccf5aa02df388d20a46097be956a662`
`0334cada6401c913081cc525d73589840d74a54a`
`ace49f01653d1f187c57679a611f57b882957e1e`
`16df13329445785a940a77641039274190f48c4d`
`3be5d92766ad4ed73208cfa41811df61d844c0ce`
`7099fe71be739b8b8f74447706a4fe4bfc27f18e`
`80bc729b93fb25a9b5904df4246ad1eb57e4bbba`
`1748596bd39a01088c7d64c4769c04a61d28a71e`
`574599fc5ca2459c2424a7d9df97095c8843cfb8`
`ceb0b269b436a7ae3518a0526fd8fb9b82cebe9a`
`f838f82d244f4698554d841d66f7c23469e54045`
`9dc57a7d9f500156996d34379240aa8e47109d97`
`3ed79b1d7bae39771fc73325ba884197de8f6034`
`6a81e25cf9f1dcf6e2eeff72c45e8b6d1a4d646f`
`3a497669b3596f24f8476f75ea8e32c66245e75b`
`0d1e5f2d5c45956b389d53eedfe053cc38c0a28f`
`3d55553b96daba87caa764d542511c29764f5eb0`
`3ec6172b73e7158c0f9555e48cd71d7dcf00b576`
`aa4b89f4b572f2fdab0dcc75b37bccf0ed5de947`
`1cc385302433d02e52ddc6b5831bc5bc484fdbfa`
`20e6b3ab22d42a7dc6a75bc6de3df831574f4ba3`
`5fcce5356c79bf4276f8fa27c89f2f807dc4ae08`
`7fca228399dd2fa4ed84cd4907e6bdedb8ba5eb0`
`21915f70f14bae69b4f765658b9c483b0e1fe613`
`2216cfe5e2929cd58f26ffa61e9c4db489bc2508`
`477a7c2e8aa50f2ad2b251b52bd7183ba004f44f`
`12af69521c00f08328e3cf97b3cf1164a74d4d45`

`Claude:claude-opus-5` (29):
`3f7bb27088781804c7e4fe9aa99e0bd7039cc178`
`1df125f883423ae11a3425682fd30e0cf1239190`
`3472d6f172d7d137a19c9793f7107b8e013ff84e`
`0e5d03dbf44eb225697f87ce64e4d4a58f955a8e`
`8a4b623d07193d5f9444fd501a2ec30fd06369a9`
`29ed936c9bc555986d019d76b1ac6dc269a286e6`
`0404c5d1103814ec08e4a290c43aa1a5ec6dc1fd`
`7c8d7b2b41a413ac4e03c6a3a9da228052192d94`
`c59ee392658d7d92cf92764f2311db6a6921110b`
`504a91c2474b3e9bfac95a85566d9cf47c64882a`
`f914384cdf066734954286795509a5bfba2fdfd6`
`13a5c799aed9d9523eda0f141853a65e9eb1d42a`
`7336a447d6763a7aae34fdc6dd9e472f940ec05c`
`a8551ed002e9fb6aca4dbc062b08096ce6cea393`
`3b84a03d3d697f4eb5b32ce271ec1872514273f5`
`6441283b1ba38f32b9514bfa9adea38d44d3e0a8`
`d948de8cdbb253fc26a2a414052e47ee658b76f2`
`aa41a948708fe5943de0faa9f6b65db4c2a54de2`
`d898fb895ddb7317e2d5238c703ac0289189eaed`
`723b8c887e7ec140587d8118e22d0ac450c8f5c6`
`6690f2eab3e27eebb684930b97a27a4fed913045`
`aa99365e0822236863440009cadf466f90e338ea`
`4b2ac7719c8a71bdfd8afdda770ea45dd0bb702a`
`b5e2b8f4263064b74145100e9a1c635ca28afdde`
`73606aa8ba5c4939327efa8d3026127ea9339243`
`bbca09fe563d01adf169c9107a63a7cf6c573cf8`
`208c83143df4110c87ca232c048fceb98c602500`
`2d0e82a03624a1b6fd3d9146a2dc14b46f1389fb`
`1748596bd39a01088c7d64c4769c04a61d28a71e`

`Copilot:deepseek-v4-pro` (23):
`0180b738f409365e63b2728759706ea491af0da9`
`fc488c18b3e2233c2b8306bc91e91e92b78af50f`
`9115f5196e878c788958511be77412dc59eff86a`
`bc396448063caa970e3b0fc5339bb22567f97ea5`
`d18baec84b34837ad8ad61360718739c5766111b`
`db460130518fbbf25d0326e2bceeef20e23027ee`
`1d42b705f05a55af64e37970534715e982d9ae77`
`8702a16b40a1050ef93a3a461f6455b387c7df12`
`41ad5f3f3a8aebe9c768c4966e93e0b919b635aa`
`22920f2ed875a8a38b1a0acd37d68c35b1e495c9`
`aa99365e0822236863440009cadf466f90e338ea`
`8e673bb58af440bba7b83c168f4b696ce94c1277`
`4b2ac7719c8a71bdfd8afdda770ea45dd0bb702a`
`b5e2b8f4263064b74145100e9a1c635ca28afdde`
`73606aa8ba5c4939327efa8d3026127ea9339243`
`bbca09fe563d01adf169c9107a63a7cf6c573cf8`
`f0831ffb6f60333f96b29b8ba871f0501d367290`
`66dfe06cbb21f584578abe14fc1ad4702e5016c3`
`fba35b22f803da632e760e8753382b721228225a`
`f772249d200318581755c2d7ea06f61b71ba1a13`
`f283c84561e19c62046610a9cb4a0de7b0413dc1`
`d678c82fd350224146d71d24eb8d260f061747ec`
`0d2385eb50a6783cf7b18332391e86acce6ca0c8`

`Kiro:GPT-5.6-Sol` (22):
`9ab6428de8fa2a640f5f55eb0d9e1f4a88520f2b`
`0180b738f409365e63b2728759706ea491af0da9`
`fc488c18b3e2233c2b8306bc91e91e92b78af50f`
`9115f5196e878c788958511be77412dc59eff86a`
`bc396448063caa970e3b0fc5339bb22567f97ea5`
`d18baec84b34837ad8ad61360718739c5766111b`
`db460130518fbbf25d0326e2bceeef20e23027ee`
`464bf7a3df760fa1c1ef42e6ea86e4075cc00e1f`
`1db24ac5bfe6ed293bc4fbb7a0342b97205a8d08`
`ee49a408d437c5b9743192bed51c05da663909f4`
`55517633a2a20ab10a062f2e8f93d21847ea0aef`
`2aef69e04530bbd61e78d716ccf89d7cb18b31b5`
`87107a0bc5dacf6c60c09f5ec19790df4b0629ce`
`333a3df42dbd28c868a6cdbea664eadbbea36747`
`09f3cc2dd02ef925512d1b8faeb81d95e951b164`
`30c96bef9fa441b41ac1e8fa96ba8726261668f0`
`3bb85da05ead1c0507e9c3cd8e0a9eb4eaec8c49`
`37717e7e140496a6e07cd057155b726dd905440f`
`224acdb69e45daaf1d77000d983e07409d491dc1`
`499a866cc42f484bacbe4837a69950cdd7a9cc6f`
`e114c807d80c84b27005bac3e9542e1081486788`
`fc5b26ba470bd8497d5b399c8df5dbadff02fd0f`

`Claude:claude-opus-4-8` (17):
`0419b1cb63b4156ba4b148f7e022f62be8d96a84`
`95a4a638917f2382ed9f68da2c78abe2714fa14f`
`4b9b00ddcaf269090eeac9957f10a60909db8302`
`2507396d10a2760be0e126107c554f7c0d46c218`
`4ca113d47f3a362963790a538f8b5c6f2964162e`
`92fc44bff8424fa59a9417cddd19e69343a37343`
`08536545757429cf5ce32ce69397d019f41959fc`
`e04e01ca5617b1e8f7edd3932b8e8ba1e6369a8d`
`4cb073ed6a587828530edf0290abf09d2c6be647`
`7bda98d5a27362e638a326e4ccb19a33561787a0`
`1fd2b0f992d25689dab23f93fb222f4f732f2937`
`827717a4e0df3ce8977c28297647c007f2df21aa`
`34ad5183b5eccc8e7bffdc15f52bf8eace8df17f`
`631f2c91c9c0d1fff35949514e40a95bde271d0c`
`742cd3affd416e6094cee54507e183cf747aaa2f`
`40ab418c5d809187817f90cecd376701b1fe8995`
`ef1556ed458bc6266cc71fa2cfdb12f8d3ec31a7`

`Claude:deepseek-v4-pro` (14):
`55bdf2b0ee4bf6f42ac4952b3eda26fdbb461781`
`df4f284e278a23b4a87346cc2415b811c0d8cc2d`
`715f23e186b728aadd4eef9d8d734a70ad8c49b8`
`8b7bc21ecb9528b9efcf6dd5316f80009fb6f5e6`
`8ac0bdf414ed2bc07398f87987684d0c5355103f`
`e6bd8eabb8b9823ae8040b602f4c6ec9c24cf19e`
`99a077806246b7eabf5b5b298a4da59d7dc934c9`
`8728b7c92e13efdb9203ab6fc77e9658d84b4cc4`
`3d55553b96daba87caa764d542511c29764f5eb0`
`2c995efa2e7d1fc8a92c0ba167c18d9a50c04f15`
`30a5bdc1e67eccab5cbd645029e3def0d89e8c6a`
`f8e2c11db77fec0f86e8036b2f50e367a8e18635`
`1c7cbd073e25d50021454378c48971391979d17a`
`3e5e4080dbe69336049c1ee18d582c821ce674be`

`Grok:4.5` (14) — R2 condition 2:
`5920042c14961a18b9f9129fcc1435fd46de777b`
`aacebf4b4b9cd520cdaad5d7eea51eb9558bcbb1`
`1afdc43b69998b713a8cfa39f493dba4ceeddce7`
`1fa8f3414385675e74d0f35998809f3373bcbc52`
`d078f8c25d010c7516810162c14b955c67373017`
`35609ef6dc0a88f8027839e793e79511308af81d`
`a9a94af6fde8986c7bbb47eeaa0498f0e5004eea`
`76766eeb6d6de7df20ac5e09872df76c5545baa9`
`36c6eacdd101189e5e72f87602f5fa2ea9a9b28d`
`41a794805a9cd368fd45019dc5cefdd5aa4b9c90`
`9b8574b43f9b66dff4f2a37d7f0910727ab347dd`
`6a2bef65408acb6fe23ce8d6414189ada7ea3275`
`86c2c5ee7b7594eb69f9a2d79faa6a23cfc6052a`
`bbca09fe563d01adf169c9107a63a7cf6c573cf8`

`Grok:grok-4.5` (14):
`9ab6428de8fa2a640f5f55eb0d9e1f4a88520f2b`
`0180b738f409365e63b2728759706ea491af0da9`
`fc488c18b3e2233c2b8306bc91e91e92b78af50f`
`9115f5196e878c788958511be77412dc59eff86a`
`bc396448063caa970e3b0fc5339bb22567f97ea5`
`d18baec84b34837ad8ad61360718739c5766111b`
`0419b1cb63b4156ba4b148f7e022f62be8d96a84`
`95a4a638917f2382ed9f68da2c78abe2714fa14f`
`4b9b00ddcaf269090eeac9957f10a60909db8302`
`6f0f98f86a1497371f691fe1b29ff2db1ee6ba6a`
`7a40ef53405ad0e983097342948f2592584e8be0`
`22e70a634f58f4374aecbbd79d14d9bacf133985`
`6f4464275ebe98ce260b9859badffaf740216ca4`
`2507396d10a2760be0e126107c554f7c0d46c218`

`Claude:claude-opus-4-6` (13):
`42ff180d5fc4d41f1999713f6bb2707cab8febc9`
`884ec49e3dcc0e982a77e3d7d331315377679962`
`aeacd8a61548c3c28256aa7ab112b5203f8163fd`
`0b94aa7bdd92ec4cf83b5be38eeb924087a3e438`
`4e2e00c5fb4a14e10f532df26383960aead70fc5`
`c3c99b623c98eb3e02f6317ba8e701c902afc01b`
`01438cd1659cdf61e78d2dbf47117234fe984c20`
`20bfd7eea585ea84f48cd3f52cc3c0575987cf68`
`eb0b46656e65882beb9645952f79c38f4e3c023b`
`c06c83cdaf8475573f464887cc55bd8329be18d0`
`fd59a22cd5e42b329ec2f8cccd0ca16497baa567`
`98c05e5a08a6de5db902e41f7155ffcf65bf9873`
`6949df987edd87e518e27b1f6bda100b6ef1aa9b`

`Kiro:Claude-Opus-5` (13):
`2a520ac799d4a6a68216588658830f6a20d826e1`
`9f8ee1a4005bcd36edcb0e3a5721840991d5931b`
`d407727051755650bedd2b6f12d781b0616cee6d`
`a696648ea714b9bd8d63684d7228f35420af2d56`
`b65faecc7c17f40f09d00feaac90016a041f6d4e`
`c96daecbb3ec33217dde084372ef7a9ea7bd2b2a`
`6c882fad59434226f8f44b68a4d12740da6bafb3`
`8fcf3d1bf7016c366602e336f07bc454bb15a937`
`516e5abf15e6033f711df08c48364da3ed249af6`
`24c84d4a0a15ba026b60fd92d21f8e61b71e1ac6`
`003c5e435abaefc02321da3be003b5cbeb93eba1`
`964372b5c308571fa527dd23af2ffb490622b4a5`
`2c7ab0a3c5a11c87342c0e2be7d483e4154fe766`

`Claude:claude-fable-5` (10):
`0180b738f409365e63b2728759706ea491af0da9`
`fc488c18b3e2233c2b8306bc91e91e92b78af50f`
`9115f5196e878c788958511be77412dc59eff86a`
`d18baec84b34837ad8ad61360718739c5766111b`
`22a97531a90e0a9193257778ece831d2a696b291`
`766131a65a1c332a36614fc82c8f8e8a265fb4ae`
`208c83143df4110c87ca232c048fceb98c602500`
`2d0e82a03624a1b6fd3d9146a2dc14b46f1389fb`
`7f9bb555b9e1c80dc81ec48c704034423b968382`
`b87785da90cea024ac95775e16bd028ffc7d5d48`

`MuseCode:muse-spark` (7):
`aa99365e0822236863440009cadf466f90e338ea`
`4b2ac7719c8a71bdfd8afdda770ea45dd0bb702a`
`b5e2b8f4263064b74145100e9a1c635ca28afdde`
`73606aa8ba5c4939327efa8d3026127ea9339243`
`bbca09fe563d01adf169c9107a63a7cf6c573cf8`
`208c83143df4110c87ca232c048fceb98c602500`
`2d0e82a03624a1b6fd3d9146a2dc14b46f1389fb`

`DeepSeek:deepseek-v4-pro` (5):
`923f35f4b6af7cc7ff3500614aaa261225f59a16`
`cc62559b653461359dc361af8abf37c6cfa368a5`
`2c5389d4bbfbf1beaf52e85d6a1dd0652ac7ffe7`
`381931de65d6f619b6387943e494cbeb4b10b2e7`
`e704f07df10727f8dab4798911c048863e0c9919`

`Copilot:muse-spark-1.2-contributor` (3):
`7a40ef53405ad0e983097342948f2592584e8be0`
`af5c490a9b61947a5fa903366e0a4aa55a263acf`
`55bd9dd1b6027d6d0684f8bbc9904c72dc5080ba`

`opencode:deepseek-v4-pro` (2):
`e7dabed19b77273de232d4f2c100b025f5e3222d`
`4a9a6f02b8012c772e94f479d6c59a3fa261632b`

`Codex:deepseek-v4-pro` (1):
`84e1770b74c5376cdb0cfa8d60f2cfe53764e448`

`Codex:gpt-5` (1):
`20e6b3ab22d42a7dc6a75bc6de3df831574f4ba3`

`Grok:4.6` (1) — R2 condition 2:
`f1e5f18c5662ff33f79c82f91443317871045144`

`opencode:muse-spark-1.2-contributor` (1):
`2507396d10a2760be0e126107c554f7c0d46c218`

Mechanical flags that are not per-value:

- Wrong key casing: none (every line is `Assisted-by:`).
- Missing colon / empty halves / placeholder / duplicate identical
  line: none.
- Multiple distinct `Assisted-by:` trailers (R2 allows this): 24
  commits — `9ab6428` `0180b73` `fc488c1` `9115f51` `bc39644`
  `d18baec` `db46013` `aa99365` `4b2ac77` `b5e2b8f` `73606aa`
  `bbca09f` `208c831` `2d0e82a` `0419b1c` `95a4a63` `4b9b00d`
  `7a40ef5` `2507396` `92fc44b` `e7dabed` `1748596` `3d55553`
  `20e6b3a`.

### Commits with no `Assisted-by:` trailer

Maintainer to annotate. The audit cannot know which of these were
AI-assisted.

| hash | date | subject |
|---|---|---|
| `08c083071a561bca8ae6a978a6015b9b8ce83f68` (`08c0830`) | 2026-08-15 | chore(deps): bump criterion from 0.5.1 to 0.8.2 |

Author is `dependabot[bot]`; the message is the stock Dependabot
bump plus `Signed-off-by: dependabot[bot] <support@github.com>`.

### Enforcement now in place

The same on-tree linter (R2) rejects `Grok:4.5` and `Grok:4.6` on any
new PR commit. It does not reject a well-shaped value the maintainer
would call the wrong model, and it does not require `Assisted-by:` on
an ordinary commit. #95 does not change R2 and adds no allowlist.

## 3. Unenforced status checks

### The rule as it should have been

A landing on `main` waits for the gates the repo treats as mandatory.
Red `cargo fmt --check` is not mergeable.

### What went wrong

[PR #98](https://github.com/shenghaoc/oxpinyin/pull/98) rebase-merged
at 2026-08-17T01:57:46Z as `12af695`. `CI / lint` (`cargo fmt --all
--check`) was already red on every recorded PR-head SHA, and the
push-to-`main` run
([31986510319](https://github.com/shenghaoc/oxpinyin/actions/runs/31986510319))
failed the same way. The diffs are rustfmt import order in
`crates/oxpinyin-capi/src/state.rs`,
`crates/oxpinyin-core/src/syllables.rs`, and
`crates/oxpinyin-core/src/vocab.rs`. `test`, both `test-portable`
legs, and `fuzz` succeeded.

The merge did not wait for those checks. The rationale at the time
was that `main` was unprotected. The tip of `main` at this audit
(`12af695`) is still fmt-red under the same three files.

Coverage of CI conclusions on `main`:

```sh
gh api --paginate \
  'repos/shenghaoc/oxpinyin/actions/workflows/ci.yml/runs?branch=main&event=push&per_page=100'
# GraphQL: repository.object(main).history statusCheckRollup.state
#   for each of the 181 commits
gh api repos/shenghaoc/oxpinyin/commits/12af69521c00f08328e3cf97b3cf1164a74d4d45/check-runs
```

181 commits queried. 147 have no check-rollup (rebase-stack
intermediates; Actions fires on the landing tip). 33 rollups are
`SUCCESS`. 1 is `FAILURE`: `12af695`. The 38 `ci.yml` push-to-`main`
runs agree: the only failure still on `main` is that tip. Four older
push runs belong to SHAs no longer on `main` (superseded pin-oracle
pushes) and are outside this catalogue.

### Enforcement now in place

Ruleset `20589678` ("Default Branch Protection"), fetched
2026-08-17:

```sh
gh api repos/shenghaoc/oxpinyin/rulesets
gh api repos/shenghaoc/oxpinyin/rulesets/20589678
gh api repos/shenghaoc/oxpinyin/rules/branches/main
```

- Target: `~DEFAULT_BRANCH` (`main`). Enforcement: `active`.
  `bypass_actors`: none.
- Created 2026-08-08T17:52:23Z; last updated 2026-08-17T02:20:08Z
  (22 minutes after the #98 merge). The API does not expose the
  pre-update rule list; the red merge is the evidence that the
  current required-check list was not enforced at 01:57Z.
- Also: no deletion, no force-push, required linear history,
  pull-request required, rebase-only, 0 approving reviews,
  `strict_required_status_checks_policy: true`.
- Required status checks (GitHub Actions, `integration_id` 15368),
  on the default branch:

  | context |
  |---|
  | `lint` |
  | `test` |
  | `test-portable (macos-latest)` |
  | `test-portable (windows-latest)` |
  | `fuzz` |

The portable matrix is in the required list.

The trailer lint is not. `.github/workflows/commit-trailers.yml`
names its jobs `lint` and `test` so they can be marked required as
`commit-trailers / lint` and `commit-trailers / test`. The ruleset
lists the bare names `lint` and `test`. `CI` uses those same job
names for rustfmt/clippy and `cargo test`. On #98 both workflows
emitted a `lint` check; `commit-trailers / lint` succeeded and
`CI / lint` failed. A required context that does not name the
workflow is not a distinct trailer-lint gate.

## Cutoff

This catalogue closes at the earlier of the ruleset tightening
(2026-08-17T02:20:08Z, ruleset `20589678`) and the commit that
introduces this file. Violations listed above remain in history on
purpose. New commits are subject to the tightened ruleset and the
on-tree trailer linter; they are not added to these tables.
