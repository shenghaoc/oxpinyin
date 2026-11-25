# oxpinyin

这是拼音输入法库 [libpinyin](https://github.com/libpinyin/libpinyin) 的可移植 Rust 重写。默认构建就是可直接替换的目标：替换拼音库的 `libpinyin.so.15`，以及 libpinyin 以 `--enable-libzhuyin` 构建的注音库 `libzhuyin.so.15`。目标是与固定版本的 libpinyin（pin）保持 bug-for-bug 兼容，连缺陷也一并复现。

[源码政策](docs/source-policy.md#stages)定义各阶段。

**状态：** Stage 2：bug-for-bug 兼容，进行中（[路线图](ROADMAP.md)）。

## 平台

仓库开发支持 x86_64 和 arm64 上的 Linux、macOS 与 Windows。
对每个改动代码的 PR，CI 会测试 Linux x86_64；macOS arm64 和 Windows x64 则每晚测试，也可手动触发。

## 性能

oxpinyin，即 #2278 的库代码（在 7e66a2e 上测得，该提交的库代码即本 PR 的库代码），与 Kali 的 libpinyin 2.11.92-1 软件包对比。两者都用 Debian 的编译选项构建：`hardening=+all`，以及 Debian 的 cargo 包装器，该包装器保留 `lto = "fat"` 和 `panic = "abort"`。所有计数均为硬件指令数（`perf_event_open`，用户态）。

| 指标 | oxpinyin | libpinyin | oxpinyin ÷ libpinyin |
|---|---:|---:|---:|
| 键入¹，指令数 | 56,290,318 | 46,849,998 | 1.20 |
| 候选路径，每个稳态周期的指令数 | 100,539,521 | 127,358,169 | 0.79 |
| 候选路径，稳态耗时² | | | 0.52 |
| 启动（冷启动打开），指令数 | 16,519,000 | 18,508,294 | 0.89 |
| 键入后内存，匿名内存 | 2,632 KiB | 2,868 KiB | 0.92 |
| 键入后内存，Pss | 14,426.5 KiB | 14,361 KiB | 1.00 |
| 安装体积，剥离后（`strip --strip-unneeded`）的 `libpinyin.so.15` | 1,262,072 B | 927,824 B | 1.36 |

¹ 键入指 `nihaozhongguo` 的每个前缀：解析、整句猜测和候选。单次完整整句猜测为 1,660,784 条指令，libpinyin 为 1,379,644（1.20）。

² 中位数之比。在这台主机上，单次运行的稳态耗时在 20 次运行之间相差 24.5%（libpinyin）和 27.0%（oxpinyin），因此表中不给出绝对耗时。

表中不给出 Callgrind 计数。Callgrind 只统计一次冷调用，其计数与硬件计数之差超出冷启动所能解释的范围，因此二者测量的不是同一事物。

编译器：在同一主机上改用 GCC 14 而非 GCC 16 构建 libpinyin 时，键入为 1.03（56,230,730 对 54,384,760，默认编译选项）。使用 GCC 14 时，`std::__lower_bound<SingleGramItem>`（+7.81 M）及其比较器 `token_less_than`（+1.75 M）变为非内联调用，`SingleGram::search` 减少 3.12 M；差异最大的十个函数解释了键入差距的 93%。

测试主机：Intel Core i7-9750H，WSL 2，Linux 6.18.58.1，amd64；Kali GNU/Linux Rolling 2026.2；GCC 16.2.0（Debian 16.2.0-4）；tkrzw 1.0.34；libpinyin pin 为 074a2219（2.11.92）。

### 其他平台，仅 oxpinyin（无 libpinyin 参照）

macOS 27.2.1，Apple M5，tkrzw 1.0.34，于 a42636b：每次操作 738,889 条指令（20 个周期 × 6 对，每次运行 2,460 次操作，指纹 `0ba4b3fa4a82f39f`；同字节对照 1.0001）。循环结束时内存占用 7,586,308 B，进程峰值 15,974,880 B。剥离后（`strip -x`）的 `libpinyin.15.0.0.dylib` 为 1,021,416 B。`leaks --atExit`：0 处泄漏。每次操作耗时 27.9 µs，各次运行之间波动最多 37%。

Windows 10.0.26300，MSVC，Intel Core i7-9750H，于 a42636b：峰值工作集 18,020,352 B，峰值私有提交 17,874,944 B（波动约 1%）；每次运行 2,460 次操作，指纹 `0ba4b3fa4a82f39f`。`oxpinyin_capi.dll` 为 1,484,800 B。不提供 CPU 数据：Windows 在没有提升权限时不提供指令计数器，而本机上的周期计数在各次运行之间波动 24–32%。

方法见 [docs/runbooks/benches.md](docs/runbooks/benches.md)。

## 快速开始

参与贡献请从 [CONTRIBUTING.md](CONTRIBUTING.md) 开始。[环境搭建指南](docs/runbooks/setup.md)介绍如何在 Linux、macOS 和 Windows 上原生构建并测试 oxpinyin。

## 目录结构

| 路径 | 内容 |
|---|---|
| `src/`、`capi/`、`zhuyin-capi/`、`utils/`、`repl/`、`tool-support/` | 工作区包；见 [crate 映射](docs/architecture.md#crates) |
| `ROADMAP.md` | 当前阶段、pin 与待完成的工作 |
| [docs/constitution.md](docs/constitution.md) | 项目规则与受支持的外部接口 |
| [docs/source-policy.md](docs/source-policy.md) | 源码阅读方法、兼容性决策与阶段规则 |
| `docs/runbooks/` | 操作指南：后端、oracle 与差分测试、Callgrind 与 Massif 配方、golden 与 pin、发布 |
| `docs/findings/` | 决策与偏离记录——已建立索引，并为每个文件标注状态 |
| `docs/testing/` | 语料库流水线、oracle 环境、fixture 格式 |
| `docs/packaging.md` | cargo-c 打包与安装树布局 |
| `tools/oracle/` | pin 构建配方（`docs/runbooks/oracle.md`） |

包的职责见 [crate 映射](docs/architecture.md#crates)。

运行或测试某个实用工具时，请选择其所在的 Cargo 包，例如：`cargo run --locked -p oxpinyin-utils --bin ngseg -- --help`。

不随产品发布的 `oxpinyin-repl` 是一个交互式命令行，用于运行库的测试模式，例如 `pinyin` 和 `parser2`。它与
`oxpinyin-utils` 一样在 Linux 和 macOS 上运行，并需要 GLib。运行
`just repl pinyin`，或 `just repl parser2 -- --parser zhuyin`。每次运行都会
把 `fixtures/w3` 复制到临时目录；训练等写入在退出后丢弃。可以用
`--backend bdb` 或 `--backend kyotocabinet` 选择后端。两个文本表模式还会复制手写的 `fixtures/datagen-toned`
文本表。

## 上游

- 行为 oracle：[libpinyin](https://github.com/libpinyin/libpinyin)——pin 见 [`tools/oracle/oracle-pin.txt`](tools/oracle/oracle-pin.txt)（[pin 说明](docs/runbooks/oracle.md#pin-identity)）；其 [wiki](https://github.com/libpinyin/libpinyin/wiki) 是上游文档（n-gram 模型格式、内部实现、多词库）
- 前端 ABI/设置接口：[ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin)——其 [wiki](https://github.com/libpinyin/ibus-libpinyin/wiki) 记录了使用方的功能与安装方法

## 许可证

GPL-3.0-or-later。
