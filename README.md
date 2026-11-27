# oxpinyin

oxpinyin is an idiomatic Rust rewrite of libpinyin that aims for close to bug-for-bug compatibility.

中文说明：[README.中文.md](README.中文.md)。

- It is a drop-in replacement for `libpinyin.so.15` (and `libzhuyin.so.15`) on Linux.
- It builds and runs natively on Linux x86_64, macOS arm64 and Windows x86_64 ([setup guide](docs/runbooks/setup.md)).
- In a comparison measured on Linux x86_64 ([docs/performance.md](docs/performance.md)), it uses less CPU and less private memory than libpinyin, about the same total memory, and a larger binary.

```mermaid
xychart-beta horizontal
    title "oxpinyin, libpinyin = 100 (lower is better)"
    x-axis ["Anonymous memory after init", "Total memory after init", "Anonymous memory after use", "Total memory after use", "Start-up time", "First key after idle, latency", "Median key time", "Worst key time", "Choose and redraw time", "Binary size"]
    y-axis "libpinyin = 100" 0 --> 130
    bar [93, 101, 82, 99, 95, 94, 90, 83, 89, 120]
    line [100, 100, 100, 100, 100, 100, 100, 100, 100, 100]
```

Linux x86_64, Kali Rolling on WSL 2. The figures, the other platforms and the method are in [docs/performance.md](docs/performance.md).

oxpinyin is based on [libpinyin](https://github.com/libpinyin/libpinyin), with the frontend ABI from [ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin).
To contribute, start at [CONTRIBUTING.md](CONTRIBUTING.md); the [roadmap](ROADMAP.md) gives the current stage.

## Licence

GPL-3.0-or-later.
