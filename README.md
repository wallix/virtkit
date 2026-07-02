# virtkit

A **rootless microVM toolkit** in two static binaries, with the VMM built in.

virtkit boots OCI/Docker images (or assembled rootfs) as fast microVMs on its
embedded [libkrun](https://github.com/containers/libkrun) VMM, connects them to a
shared LAN whose egress travels through the host's ordinary sockets, and drives
commands into them over `vsock` — all of it as a plain user process.
[Cloud Hypervisor](https://www.cloudhypervisor.org/) is also supported as an
external backend (`VIRTKIT_VMM=cloud-hypervisor`).

From one codebase it powers a local **dev fleet** (a dev VM + service VMs, like
docker-compose but as VMs) and a **GitLab custom executor** (one throwaway VM per
CI job) — and the pieces (image building, the network switch, the in-VM agent)
are usable on their own.

## A taste

```sh
# boot an image and step inside
vk run alpine:latest --shell

# compile the current tree in a throwaway microVM: /work is this directory,
# so target/ lands back on the host
vk run rust:1-alpine --workdir . --net --cpus host --mem 4G -- cargo build --release

# build a Dockerfile (each RUN in its own microVM, instruction-cached),
# boot the resulting image and run a command in it
vk run -f Dockerfile --net -- ./run-tests.sh
```

## The two binaries

| Binary | Role |
| --- | --- |
| `vk` | **host driver** — the libkrun VMM, image building/conversion, the fleet orchestrator + control plane, the GitLab executor, the userspace network switch, and a bundled virtio-fs daemon. Ships self-contained: the pinned guest kernel and `vk-agent` are embedded, so one file boots images. |
| `vk-agent` | **guest PID 1 / agent** — brings a systemd-less guest up (mounts, networking, hostname, virtio-fs, optional SSH), and serves an exec channel over `vsock` so the host can run commands inside the VM. |

## Features

- **The VMM is built in.** Guests boot on the embedded libkrun; everything runs
  with ordinary user privileges, on a stock kernel and stock KVM.
- **Userspace networking.** A userspace L2 switch (ARP + DHCP + a DNS gateway,
  with transparent TCP/UDP egress via
  [`ipstack`](https://crates.io/crates/ipstack)) carries guest traffic over
  `vsock` and hands it to the host's regular sockets — the whole data path lives
  inside the `vk` process.
- **microVM fleet.** Boot a dev VM + service VMs (redis, mysql, …) on one shared
  `*.lan` network; start/stop them on demand with the in-VM `virtctl` client
  (expose it with `fleet --vm-symlink /usr/local/bin/vk-agent:/usr/local/bin/virtctl`).
- **GitLab CI executor.** A custom executor that boots a fresh microVM per job,
  runs each stage over `vsock`, and tears it down — with a tap pool for concurrent
  jobs and on-demand OCI-image conversion.
- **Content-addressed images.** `mkext-tar` streams a `docker export` straight
  into a native ext4 image, entirely in userspace; the filesystem UUID is a
  fingerprint of the build inputs, so staleness is a UUID compare.
- **Batteries included.** The VMM (vendored libkrun), the guest kernel
  (`build-kernel.sh`, embedded into `vk`) and a vhost-user virtio-fs daemon
  (`vk virtiofsd`, serving cloud-hypervisor shares with the same libkrun fs
  engine) all ship inside virtkit. `vk` can even rebuild itself inside one of its
  own microVMs (`./build.sh --bootstrap-check`).
- **Reproducible builds.** Static-musl binaries from a digest-pinned Alpine
  toolchain with pinned apk versions; `./update.sh` records the pins.

## Build

```sh
./build.sh         # -> dist/{vk, vk-agent, *.sha256, build-info.txt}
./build-kernel.sh  # -> dist/vmlinux (the guest kernel; rebuilt only on a pin bump)
```

Both run inside a pinned `rust:*-alpine` container (Docker required), so the
artifacts are byte-reproducible regardless of host. `./update.sh` bumps the Rust
toolchain, the base-image digest and the apk pins together.

## Subcommands

`vk`:

- `run` — boot an image (or a Dockerfile target) as a microVM and run a command
  or an interactive shell in it.
- `fleet` — orchestrate the dev fleet (dev VM + service VMs on one LAN).
- `gitlab config` / `gitlab prepare` / `gitlab run` / `gitlab cleanup` — the GitLab custom-executor lifecycle.
- `build` — build a Dockerfile into a bootable ext4, each `RUN` in a microVM.
- `switch` — the userspace L2 network gateway (run in-process by `fleet`).
- `mkext-tar` / `mkext` — build an ext4 image from a rootfs tar / directory.
- `oci-pull` — pull and flatten an OCI image to a rootfs tar.
- `registry push` / `registry pull` — push/pull guest bundles to/from an OCI registry
  with content-defined chunk dedup (CDC + per-chunk zstd).
- `virtiofsd` — the bundled vhost-user virtio-fs daemon (cloud-hypervisor shares).
- `forward` / `launch` — byte forwarder / standalone microVM launcher.

`vk-agent`:

- `init` — PID 1 for a systemd-less guest (also runs the captured entrypoint /
  hands off to systemd, depending on `VIRTKIT_MODE`).
- `serve` — the exec server (`vsock`); `exec` / `connect` / `forward` are the host-
  side clients (e.g. `connect` is an SSH `ProxyCommand` over `vsock`).
- `net` — bridge a guest tap NIC to the host switch over `vsock`.
- Invoked as `virtctl` (a symlink exposed via `fleet --vm-symlink`), it is the fleet
  control client (`virtctl start <unit>`, …).

## Layout

```
vk-core/         shared host↔guest library (wire protocol + exec/pty/dockerignore)
vk-driver/       host driver crate
vk-agent/        guest agent crate (PID 1 + exec server)
third_party/     vendored libkrun (locally patched — see its VENDOR.md)
kernel/          guest kernel build (Dockerfile + config fragment)
build.sh         build the binaries -> dist/
build-kernel.sh  build the guest kernel -> dist/vmlinux
update.sh        bump + re-pin toolchain / base image / apk versions
```

## License

Copyright © Wallix. See [LICENSE](LICENSE).
