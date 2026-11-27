# oxpinyin

oxpinyin 是 libpinyin 的地道 Rust 重写，目标是接近 bug-for-bug 兼容。

English: [README.md](README.md).

- 它在 Linux 上可直接替换 `libpinyin.so.15`（以及 `libzhuyin.so.15`）。
- 它在 Linux x86_64、macOS arm64 和 Windows x86_64 上原生构建并运行（[环境搭建指南](docs/runbooks/setup.md)）。
- 在 Linux x86_64 上测得的对比中（[docs/performance.md](docs/performance.md)），它比 libpinyin 占用更少的 CPU 和匿名内存，常驻内存大致相同，二进制文件更大。

| 与 libpinyin 相比 | oxpinyin |
|---|---|
| 使用后匿名内存 | **少 18%** |
| 常驻内存（PSS） | 基本持平 |
| 按键耗时中位数 | **少 10%** |
| 最慢按键耗时 | **少 17%** |
| 选词并重绘耗时 | **少 11%** |
| 启动耗时 | **少 5%** |
| 二进制大小 | 大 20% |

Linux x86_64。数据与测量方法：[docs/performance.md](docs/performance.md)。

oxpinyin 基于 [libpinyin](https://github.com/libpinyin/libpinyin)，前端 ABI 来自 [ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin)。
参与贡献请从 [CONTRIBUTING.md](CONTRIBUTING.md) 开始；当前阶段见[路线图](ROADMAP.md)。

## 许可证

GPL-3.0-or-later。
