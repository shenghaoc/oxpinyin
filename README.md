# oxpinyin

oxpinyin is an idiomatic Rust rewrite of libpinyin that aims for close to bug-for-bug compatibility.

中文说明：[README.中文.md](README.中文.md)。

- It is a drop-in replacement for `libpinyin.so.15` (and `libzhuyin.so.15`) on Linux.
- It builds and runs natively on Linux x86_64, macOS arm64 and Windows x86_64 ([setup guide](docs/runbooks/setup.md)).
- In a comparison measured on Linux x86_64 ([docs/performance.md](docs/performance.md)), it uses less CPU and less anonymous memory than libpinyin, about the same resident memory, and a larger binary.

| Compared with libpinyin | oxpinyin |
|---|---|
| Anonymous memory after use | **18% less** |
| Resident memory (PSS) | about the same |
| Median key time | **10% less** |
| Worst key time | **17% less** |
| Choose and redraw | **11% less** |
| Start-up time | **5% less** |
| Binary size | 20% larger |

Linux x86_64. Figures and method: [docs/performance.md](docs/performance.md).

oxpinyin is based on [libpinyin](https://github.com/libpinyin/libpinyin), with the frontend ABI from [ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin).
To contribute, start at [CONTRIBUTING.md](CONTRIBUTING.md); the [roadmap](ROADMAP.md) gives the current stage.

## Install

The Linux release packages are on [GitHub Releases](https://github.com/shenghaoc/oxpinyin/releases).
Each package is for one distribution and one storage backend:

| Distribution | Backend | Distribution packages replaced |
|---|---|---|
| Debian testing | tkrzw | `libpinyin15`, `libzhuyin15`, `libpinyin15-dev`, `libzhuyin-dev` |
| Fedora Rawhide | Kyoto Cabinet | `libpinyin`, `libzhuyin`, `libpinyin-devel` |
| Arch | Kyoto Cabinet | `libpinyin` |

The packages replace the distribution packages in the last column.
The Debian packages are built with tkrzw for the data of Debian testing. They do not support Debian stable, whose libpinyin 2.8.1 uses Berkeley DB.
[Packaging](docs/packaging.md#release-artifacts-release-packagesyml) gives the detail.

## Licence

GPL-3.0-or-later.
