# systemd-top

Rust terminal monitor for systemd units. Installed binary: `/usr/local/bin/systemd-top`.

```sh
systemd-top
systemd-top -d 1 -s memory
systemd-top -s time --filter nginx
systemd-top --user --all
systemd-top --type scope
systemd-top --once --raw
systemd-top --help
```

`-d` specifies seconds (including fractions). Values normally display hours/minutes/seconds and binary memory units. `--raw` displays nanoseconds and bytes. `--once` prints a tab-separated snapshot; CPU% needs two samples and is unavailable in this mode.

Keys: arrows, Page Up/Down, Home/End navigate; F1 or ? opens help; F2 toggles raw units; F3/F4 or / edits the filter (Enter applies, Escape clears); F5 refreshes; F6 or s cycles sorting; r reverses; Space pauses; F10 or q quits. Direct sorting: c CPU%, t CPU time, m memory, n name, [ minimum limit, ] maximum limit.

CPU time is cumulative accounting for the unit, including its child processes. CPU% measures changes between samples; 100% is one CPU core. Memory is MemoryCurrent; minimum and maximum are MemoryMin and MemoryMax configuration, not historical extrema. TasksCurrent includes threads. An em dash means unavailable; infinity means an unlimited maximum. Enable CPUAccounting/MemoryAccounting/TasksAccounting on your units if metrics are missing. The monitor only reads systemd properties and does not change unit configuration.

Common htop-style flags and shortcuts are supported as listed above; this is not a drop-in implementation of every htop option. `-u` selects the user systemd manager rather than a process owner. `-C` accepts monochrome mode (the interface is monochrome by default). Filtering selects unit names.

Build: `cargo test && cargo build --release`. Install: `install -m 755 target/release/systemd-top /usr/local/bin/systemd-top`. Clean: `cargo clean`.
