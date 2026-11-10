# oxpinyin

这是拼音输入法库 [libpinyin](https://github.com/libpinyin/libpinyin) 的可移植 Rust 重写。默认构建就是可直接替换的目标：替换拼音库的 `libpinyin.so.15` 和注音库的 `libzhuyin.so.15`。目标是与固定版本的 libpinyin（pin）保持 bug-for-bug 兼容，连缺陷也一并复现。尚未做到完整的 bug-for-bug 兼容。

Stage 3 构建需显式启用。只改变可观察输出的 Stage 3 构建保留 ABI 和数据格式，并在构建元数据中声明偏离之处。改变 ABI 或原生数据格式的 Stage 3 构建不得使用 libpinyin 的 SONAME、pkg-config 文件或头文件目录，除非维护者裁决允许。

[源码政策](docs/source-policy.md#stages)定义各阶段，[路线图](ROADMAP.md)列出待完成的工作。

**状态：** Stage 1 已完成：1,571/1,571 条语料库记录，79/79 个导出符号。已在 Fedora rawhide、Debian testing 和 NixOS 上验证可作为 `libpinyin.so.15` 直接替换。当前处于 Stage 2，其收尾门禁是第三轮 bug-for-bug 排查（[路线图](ROADMAP.md)）。

## 平台

可移植的 crate 支持 x86_64 和 arm64 上的 Linux、macOS 与 Windows。对每个改动代码的 PR，CI 会测试 Linux x86_64；macOS arm64 和 Windows x64 则每晚测试，也可手动触发。

## 快速开始

从 [CONTRIBUTING.md](CONTRIBUTING.md) 开始。其中依次介绍前置条件、Linux/macOS/Windows 原生环境搭建、第一次链接并运行测试，以及如何在新终端中继续工作。日常开发不需要 oracle、model20 或容器。兼容性门禁请使用已配置的[远程 Linux 主机](docs/runbooks/cloud.md)。

[环境搭建指南](docs/runbooks/setup.md#storage-backend)给出默认的存储后端，以及各平台所需的软件包和设置。

## 目录结构

| 路径 | 内容 |
|---|---|
| `crates/*` | 工作区包；见 [crate 映射](docs/architecture.md#crates) |
| `ROADMAP.md` | 当前阶段、pin 与待完成的工作 |
| [docs/constitution.md](docs/constitution.md) | 项目规则与受支持的外部接口 |
| [docs/source-policy.md](docs/source-policy.md) | 源码阅读方法、兼容性决策与阶段规则 |
| `docs/runbooks/` | 操作指南：后端、oracle 与差分测试、Callgrind 与 Massif 配方、golden 与 pin、发布 |
| `docs/findings/` | 决策、偏离记录、审计轨迹——已建立索引，并为每个文件标注状态 |
| `docs/testing/` | 语料库流水线、oracle 环境、fixture 格式 |
| `docs/packaging.md` | cargo-c 打包与安装树布局 |
| `tools/oracle/` | pin 构建配方（`docs/runbooks/oracle.md`） |

包的职责见 [crate 映射](docs/architecture.md#crates)。

保留下来的各阶段二进制保持其名称和参数。运行或测试这些命令时，请选择整合后的 Cargo 包，例如：`cargo run --locked -p oxpinyin-utils --bin ngseg -- --help`。

不随产品发布的 `oxpinyin-repl` 整合了上游十个交互测试模式。它与
`oxpinyin-utils` 一样在 Linux 和 macOS 上运行，并需要 GLib。运行
`just repl pinyin`，或 `just repl parser2 -- --parser zhuyin`。每次运行都会
把 `fixtures/w3` 复制到临时目录；训练等写入在退出后丢弃。可以用
`--backend bdb` 或 `--backend kyotocabinet` 选择后端。21 个实用工具
仍保留其兼容接口。两个文本表模式还会复制手写的 `fixtures/datagen-toned`
文本表。

## 上游

- 行为 oracle：[libpinyin](https://github.com/libpinyin/libpinyin)——pin 见 [`tools/oracle/oracle-pin.txt`](tools/oracle/oracle-pin.txt)（[pin 说明](docs/runbooks/oracle.md#pin-identity)）；其 [wiki](https://github.com/libpinyin/libpinyin/wiki) 是上游文档（n-gram 模型格式、内部实现、多词库）
- 前端 ABI/设置接口：[ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin)——其 [wiki](https://github.com/libpinyin/ibus-libpinyin/wiki) 记录了使用方的功能与安装方法

## 许可证

GPL-3.0-or-later。
