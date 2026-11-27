# Performance

This page is for integrators and packagers. It gives measured resource use only.
The methods are in [benches.md](runbooks/benches.md).

## Linux x86_64, against libpinyin

- Kali Rolling is used because it is a rolling distribution that is available on WSL, so the Linux and Windows figures come from one machine.
- WSL 2 runs a real Linux kernel, and the instruction counts come from `perf_event_open` there, in user space.
- The pin is Kali's own `libpinyin15` package, built with Kali's toolchain and flags.
- Typing uses sort `0x1c`, ibus-libpinyin's default.
- The libpinyin comparison covers Linux x86_64 only.

| | oxpinyin | libpinyin |
| --- | --- | --- |
| Source | `c6b8e1e23` | `libpinyin15` 2.11.92-1, with `libpinyin-data` 2.11.92-1 |
| Build | `cargo cinstall` with Debian's flags ([build oxpinyin with the flags](runbooks/benches.md#build-oxpinyin-with-the-flags)), tkrzw | Kali's package |
| Data | the package's data | the package's data |

Host: Intel Core i7-9750H, 15.8 GiB, Windows 11 Home 10.0.26300; WSL 2 with kernel 6.18.58.1-microsoft-standard-WSL2, 12 logical CPUs and 7.7 GiB.
Kali GNU/Linux Rolling 2026.2, GCC 16.2.0 (Debian 16.2.0-4), glibc 2.43-4, GLib 2.90.1-2, tkrzw 1.0.34-1, Rust 1.98.1, cargo-c 0.10.25, gdb 17.2.

The [output-identity gate](runbooks/benches.md#output-identity-gate) and the [reproduction check](runbooks/benches.md#reproduction-check) passed.
The [resource figures](runbooks/benches.md#resource-figures) section defines each measure.
Each figure is the median of 7 rounds of fresh processes (memory: 3 rounds).
A range in parentheses is the range over the rounds; it is given where it is wider than 1%.
A per-key figure is the median of all its samples in the 7 rounds, 343 for each key.

| Measure | oxpinyin | libpinyin | oxpinyin ÷ libpinyin |
| --- | ---: | ---: | ---: |
| **Idle memory after init: nothing typed** | | | |
| Anonymous | 1,388 KiB | 1,488 KiB | 0.93 |
| Private_Dirty | 1,388 KiB | 1,488 KiB | 0.93 |
| PSS | 9,955 KiB (9,954–9,968) | 9,834 KiB (9,820–9,844) | 1.01 |
| `[stack]` row, Rss | 20 KiB | 12 KiB | 1.67 |
| Library text row, Rss | 788 KiB | 524 KiB | 1.50 |
| **Idle memory after use: typed, chosen and trained** | | | |
| Anonymous | 2,220 KiB | 2,712 KiB | 0.82 |
| Private_Dirty | 2,220 KiB | 2,712 KiB | 0.82 |
| PSS | 19,907 KiB (19,902–19,913) | 20,099 KiB (20,089–20,110) | 0.99 |
| `[stack]` row, Rss | 20 KiB | 12 KiB | 1.67 |
| Library text row, Rss | 788 KiB | 524 KiB | 1.50 |
| **Start-up: `pinyin_init` to the first candidate list** | | | |
| Instructions | 9,256,270 | 11,057,626 | 0.84 |
| Wall time, warm | 3.56 ms | 3.74 ms | 0.95 |
| **First key after idle: library and data paged out** | | | |
| Latency | 33.3 ms (25.9–50.3) | 31.2 ms (23.5–39.8) | 1.07 |
| Major faults | 106 | 103 | 1.03 |
| Instructions | 7,158,160 | 7,200,281 | 0.99 |
| Latency without the page-out | 1.71 ms (1.63–1.93) | 2.05 ms (1.90–2.47) | 0.83 |
| **Typing: each prefix of `nihaozhongguo`** | | | |
| Median key, instructions | 3,203,348 | 3,730,913 | 0.86 |
| Median key, wall time | 1.54 ms | 1.61 ms | 0.95 |
| Worst key (`nihaozhon`), instructions | 7,442,529 | 8,428,815 | 0.88 |
| Worst key (`nihaozhon`), wall time | 2.14 ms | 2.42 ms | 0.88 |
| Whole sequence, instructions | 45,728,683 | 51,695,107 | 0.88 |
| Whole sequence, wall time | 17.8 ms (16.4–18.5) | 20.7 ms (18.3–21.5) | 0.86 |
| **Selection (`--perf-choose`)** | | | |
| Choose and redraw, instructions | 1,512,480 | 1,554,493 | 0.97 |
| Choose and redraw, wall time | 335 µs (322–365) | 376 µs (350–402) | 0.89 |
| Choose and train, instructions | 54,004 | 72,198 | 0.75 |
| Choose and train, wall time | 92 µs (91–101) | 110 µs (104–117) | 0.83 |
| Save, instructions | 104,592,211 | 105,169,496 | 0.99 |
| Save, wall time | 106.7 ms (105.5–109.6) | 106.9 ms (105.2–109.4) | 1.00 |
| **Full-sentence guess (`--perf-sentence`)** | | | |
| Instructions | 1,185,549 | 1,342,189 | 0.88 |
| Wall time | 1.12 ms (1.08–1.17) | 1.22 ms (1.13–1.27) | 0.92 |
| **Size** | | | |
| Stripped `libpinyin.so.15` | 1,135,024 B | 927,824 B | 1.22 |

The first-key-after-idle difference is within the run-to-run variation: the ranges of the two latencies overlap.

The [README](../README.md) chart shows the wall times and both idle memory points, with libpinyin = 100.
Its "total memory" is PSS.

## macOS arm64, oxpinyin only

Measured at `b221f3312` with [`just measure compare`](runbooks/benches.md#native-macos-source-revisions).

| Measure | oxpinyin |
| --- | ---: |
| Instructions per operation | 698,280 |
| Time per operation | 27.44 µs (27.27–28.77) |
| Footprint at loop end | 7,373,304 B |
| Lifetime peak footprint | 15,761,888 B |
| Peak RSS | 21,725,184 B |
| Stripped (`strip -x`) `libpinyin.15.0.0.dylib` | 971,816 B |
| `leaks --atExit` | 0 leaks |

Host: Mac17,2, Apple M5, 16 GB, macOS 27.2.1 (26B131), Xcode 27.2, tkrzw 1.0.34 (Homebrew), rustc 1.98.1.

## Windows x86_64, oxpinyin only

Measured at `c6b8e1e23` with [`just measure compare`](runbooks/benches.md#native-windows-comparisons), base and head at that commit: 24 runs of 20 cycles, 2,460 operations each, on Debian's `libpinyin-data` 2.11.92-1.

| Measure | oxpinyin |
| --- | ---: |
| Peak working set | 18,071,552 B (17,985,536–18,288,640) |
| Peak private commit | 17,891,328 B (17,784,832–18,030,592) |
| Expected commit from copy-on-write views of the data | 4,177,920 B |
| `oxpinyin_capi.dll` | 1,409,536 B |

There is no CPU figure. Windows gives no instruction counter without elevated privileges.
The CPU-cycle count of the loop (`QueryProcessCycleTime`) changes with clock speed and contention: it varied by 23% over the 24 runs of one binary.

Host: the machine of the [Linux figures](#linux-x86_64-against-libpinyin), natively.
MSVC 14.44.35207, Rust 1.98.1, GLib 2.90.1 (vcpkg), tkrzw 1.0.34 (static, `just tkrzw`).
