# Vendored libkrun

Source: https://github.com/containers/libkrun
Revision: `9a8fedc7fa425a36ae978d529a6c0dc7124efe7d` (stable-1.19.x, carries PR #728)

Only the Rust sources are vendored: `Cargo.toml`, `Cargo.lock`, `LICENSE`, and
`src/`. The upstream `.git`, `examples/`, `tests/`, `docs/`, and `edk2/` are dropped
(the `libkrun` crate does not need them for the `blk` + `net` Linux build virtkit uses).

This is its own cargo workspace, excluded from the root virtkit workspace. The host
crate depends on `src/libkrun` (package `libkrun`, lib name `krun`) as a path
dependency, so it shares virtkit's `std` — avoiding the double-std / broken-unwinding
that a static `libkrun.a` link hits.

## Local patch

`src/devices/src/virtio/fs/linux/passthrough.rs` — the passthrough fs device called
`libc::statx` with `libc::STATX_BASIC_STATS | libc::STATX_MNT_ID`. libc dropped its
musl `statx` struct/fn/constants after 0.2.183, but virtkit needs a newer libc (its
dependency tree pulls libc >= 0.2.186). `struct statx` is defined by the kernel UAPI
to be architecture-independent, so the patch reproduces exactly the fields the device
reads and issues the raw `SYS_statx` syscall. Behaviour is identical, including the
returned `stx_mnt_id`. Search for `mod statx_compat` in that file.
