# oxpinyin

这是拼音输入法库 [libpinyin](https://github.com/libpinyin/libpinyin) 的可移植 Rust 重写。默认构建就是可直接替换的目标：替换拼音库的 `libpinyin.so.15` 和注音库的 `libzhuyin.so.15`。目标是与固定版本的 libpinyin（pin）保持 bug-for-bug 兼容，连缺陷也一并复现。尚未做到完整的 bug-for-bug 兼容。

Stage 3 构建需显式启用。只改变可观察输出的 Stage 3 构建保留 ABI 和数据格式，并在构建元数据中声明偏离之处。改变 ABI 或原生数据格式的 Stage 3 构建不得使用 libpinyin 的 SONAME、pkg-config 文件或头文件目录，除非维护者裁决允许。

- Stage 1（已完成）：在各项语料测试中，输出与 pin 完全一致。
- Stage 2（当前）：在整个导出 ABI 范围内保持 bug-for-bug 兼容，同时进行内部改进。只要兼容性保持不变，就允许开展性能工作。该阶段尚未完成。未完成的工作由带 `bug-for-bug` 标签的开放 issue 跟踪；当前跟踪 issue 为 #870。
- Stage 3（下一步）：需显式启用的、对 pin 的偏离，例如新的语言模型或不兼容的数据格式。默认构建仍保持 bug-for-bug 兼容。

**状态：** Stage 1 已完成——1,571/1,571 条语料库记录；79/79 个导出符号；已在 Fedora rawhide、Debian testing 和 NixOS 上验证可作为 `libpinyin.so.15` 直接替换。当前处于 Stage 2（bug-for-bug）。第二轮审计已于 2026-10-17 结束（#573）。待完成的工作是初始化复杂度跟踪 issue（#870），之后是第三轮 bug-for-bug 排查（`ROADMAP.md`）。保持兼容性的性能工作仍在 Stage 2 继续（`docs/perf/`）。随后是 Stage 3（需显式启用的偏离）。

## 平台

可移植的 crate 支持 x86_64 和 arm64 上的 Linux、macOS 与 Windows。对每个改动代码的 PR，CI 会测试 Linux x86_64；macOS arm64 和 Windows x64 则每晚测试，也可手动触发。

## 快速开始

从 [CONTRIBUTING.md](CONTRIBUTING.md) 开始。其中依次介绍前置条件、Linux/macOS/Windows 原生环境搭建、第一次链接并运行测试，以及如何在新终端中继续工作。日常开发不需要 oracle、model20 或容器。兼容性门禁请使用已配置的[远程 Linux 主机](docs/runbooks/cloud.md)，或[规范的 Debian trixie 兼容性容器](docs/runbooks/compatibility-container.md)；后者会把被测实现、参考实现和对比测试框架放在一起运行。

[环境搭建指南](docs/runbooks/setup.md#storage-backend)给出默认的存储后端，以及各平台所需的软件包和设置。

## 目录结构

| 路径 | 内容 |
|---|---|
| `crates/*` | 20 个工作区包，没有占位包（分组见下）；crate 映射见 `docs/architecture.md` |
| `ROADMAP.md` | 阶段、pin、工作流名称 |
| [docs/constitution.md](docs/constitution.md) | 项目规则与受支持的外部接口 |
| [docs/source-policy.md](docs/source-policy.md) | 源码阅读方法、兼容性决策与阶段规则 |
| `docs/runbooks/` | 操作指南：后端、oracle 与差分测试、基准测试、golden 与 pin、发布 |
| `docs/findings/` | 决策、偏离记录、审计轨迹——已建立索引，并为每个文件标注状态 |
| `docs/perf/` | 性能测量快照 |
| `docs/testing/` | 语料库流水线、oracle 环境、fixture 格式 |
| `docs/safety/` | MISRA-Rust 映射、审计、安全规范 |
| `docs/packaging.md` | cargo-c 打包与安装树布局 |
| `docs/oxidation-journey.md` | 迄今为止将 libpinyin 转写为 Rust 的历程，以及由此总结出的方法 |
| `tools/oracle/` | pin 构建配方（`docs/runbooks/oracle.md`） |
| [docs/specs/](docs/specs/) | 基础、直接替换兼容性、Rust 转写与 Stage 3 的需求、设计和任务记录 |

| 分组 | crate |
|---|---|
| 引擎 / 数据 / 运行时 / 绑定 | `oxpinyin-core`、`oxpinyin-chewing`、`oxpinyin-store`、`oxpinyin-data`、`oxpinyin-user`、`oxpinyin-engine`、`oxpinyin-facade`、`oxpinyin-runtime`、`oxpinyin-capi`、`oxpinyin-zhuyin-capi`、`oxpinyin-capi-marshal` |
| 训练工具链（不随产品发布） | `oxpinyin-training`（模块：`segment`、`counter`、`lambda`、`emitter`、`eval`；二进制：`oxpinyin-segment`、`oxpinyin-spseg`、`oxpinyin-mergeseq`、`oxpinyin-counter`、`oxpinyin-lambda`、`oxpinyin-emitter`）、`oxpinyin-kmm`、`oxpinyin-utils`、`oxpinyin-datagen` |
| 工具 | `oxpinyin-dictool` |
| Oracle / 测试 | `pinyin-oracle`、`oxpinyin-testsupport` |

保留下来的各阶段二进制保持其名称和参数。运行或测试这些命令时，请选择整合后的 Cargo 包，例如：`cargo run --locked -p oxpinyin-training --bin oxpinyin-segment -- --help`。

## 上游

- 行为 oracle：[libpinyin](https://github.com/libpinyin/libpinyin)——pin 见 `docs/testing/oracle-environment.md`；其 [wiki](https://github.com/libpinyin/libpinyin/wiki) 是上游文档（n-gram 模型格式、内部实现、多词库）
- 前端 ABI/设置接口：[ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin)——其 [wiki](https://github.com/libpinyin/ibus-libpinyin/wiki) 记录了使用方的功能与安装方法

## 许可证

GPL-3.0-or-later。
