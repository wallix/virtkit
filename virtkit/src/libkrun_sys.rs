//! libkrun backend: the `__libkrun-boot` subcommand drives libkrun's C API to
//! boot a [`VmSpec`] in this process. libkrun is the vendored `krun` rlib crate
//! (third_party/libkrun), so it shares virtkit's std — no static-`libkrun.a`
//! double-std to reconcile.
//!
//! libkrun runs as a per-VM subprocess (the [`crate::vmm::Libkrun`] impl execs
//! `vk __libkrun-boot <spec-json>`), so it slots into the same lifecycle as the
//! cloud-hypervisor backend — held `Child` / `spawn_tied`, no in-process VMM in
//! the orchestrator. We always supply our own kernel via `krun_set_kernel`, so
//! libkrun never loads libkrunfw (see lib.rs:2848 upstream): the bundled-kernel
//! `.so` is neither linked nor needed.
//!
//! Increment 1 scope: boot a disk/initramfs guest with our kernel + cmdline-`init=`
//! (PID 1) and the console on our stdio. vsock exec, virtio-fs shares, networking
//! and the shutdown eventfd are wired in later increments.

use std::ffi::CString;

use anyhow::{Result, bail};

// libkrun's C-ABI entry points, called directly from the linked `krun` crate
// (rlib -> shares virtkit's std; compiler-checked signatures). Every call returns
// >= 0 on success, a negative errno on failure.
use krun::{
    krun_add_disk2, krun_add_virtio_console_default, krun_create_ctx, krun_disable_implicit_init,
    krun_init_log, krun_set_kernel, krun_set_vm_config, krun_start_enter,
};

use crate::vmm::{Disk, VmSpec};

const KRUN_KERNEL_FORMAT_ELF: u32 = 1;
const KRUN_DISK_FORMAT_RAW: u32 = 0;
const KRUN_DISK_FORMAT_QCOW2: u32 = 1;

/// Check a libkrun call's return: `>= 0` ok, negative errno on failure.
fn ck(what: &str, rc: i32) -> Result<()> {
    if rc < 0 {
        bail!("{what} failed: rc={rc} (errno {})", -rc);
    }
    Ok(())
}

fn cstr(s: &str) -> CString {
    CString::new(s).expect("nul byte in libkrun argument")
}

/// Parse a memory size token (`"8G"`) into MiB for `krun_set_vm_config`.
fn mem_mib(mem: &str) -> Result<u32> {
    let n: u32 = mem
        .strip_suffix('G')
        .and_then(|g| g.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("memory size {mem:?} is not <n>G"))?;
    n.checked_mul(1024)
        .ok_or_else(|| anyhow::anyhow!("memory size {mem:?} overflows MiB"))
}

/// Boot `spec` under libkrun in this process. Returns only when the guest powers
/// off (or never, until then) — the caller is the `__libkrun-boot` subprocess.
pub fn boot(spec: &VmSpec) -> Result<()> {
    unsafe {
        // verbose to stderr while the backend is young; quieten once it's the default.
        krun_init_log(2, 4, 0, 0);

        let ctx = krun_create_ctx();
        ck("krun_create_ctx", ctx)?;
        let ctx = ctx as u32;

        ck(
            "krun_set_vm_config",
            krun_set_vm_config(ctx, spec.cpus as u8, mem_mib(&spec.mem)?),
        )?;

        // guest console (hvc0) -> our stdio (required in libkrun 2.0; 1.x auto-added one).
        ck(
            "krun_add_virtio_console_default",
            krun_add_virtio_console_default(ctx, 0, 1, 2),
        )?;

        // our own kernel + cmdline; PID 1 is chosen by `init=` on the cmdline.
        let kernel = cstr(&spec.kernel.to_string_lossy());
        let cmdline = cstr(&spec.cmdline);
        let initramfs = spec
            .initramfs
            .as_ref()
            .map(|p| cstr(&p.to_string_lossy()));
        ck(
            "krun_set_kernel",
            krun_set_kernel(
                ctx,
                kernel.as_ptr(),
                KRUN_KERNEL_FORMAT_ELF,
                initramfs.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                cmdline.as_ptr(),
            ),
        )?;

        // virtio-blk disks in order (first = /dev/vda). qcow2 overlays resolve their
        // backing chain (KRUN_DISK_FORMAT_QCOW2); raw bases use KRUN_DISK_FORMAT_RAW.
        for (i, disk) in spec.disks.iter().enumerate() {
            add_disk(ctx, i, disk)?;
        }

        // our cmdline's init= is PID 1; don't let libkrun inject /init.krun.
        ck("krun_disable_implicit_init", krun_disable_implicit_init(ctx))?;

        // blocks until the guest powers off.
        ck("krun_start_enter", krun_start_enter(ctx))?;
    }
    Ok(())
}

unsafe fn add_disk(ctx: u32, index: usize, disk: &Disk) -> Result<()> {
    let block_id = cstr(&format!("vd{}", (b'a' + index as u8) as char));
    let path = cstr(&disk.path.to_string_lossy());
    let format = if disk.qcow2 {
        KRUN_DISK_FORMAT_QCOW2
    } else {
        KRUN_DISK_FORMAT_RAW
    };
    ck(
        "krun_add_disk2",
        unsafe { krun_add_disk2(ctx, block_id.as_ptr(), path.as_ptr(), format, disk.readonly) },
    )
}
