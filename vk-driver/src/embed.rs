//! The guest kernel and vk-agent, optionally embedded into the `vk` binary.
//!
//! The shipped `vk` is self-contained: build.sh compiles it with the `embed`
//! feature and points build.rs at a freshly built vk-agent and the pinned vmlinux.
//! At runtime an asset is resolved in this order: an explicit --kernel/--agent
//! path, else the embedded copy (written into the launch's scratch dir), else the
//! on-disk default under /usr/local/lib/vk. A plain dev `cargo build` embeds
//! nothing, so it just uses the flags or the on-disk defaults.
//!
//! The blobs are embedded with the linker (`.incbin` in a `global_asm!` section),
//! not `include_bytes!`: the linker splices each file straight into `.rodata`, so
//! rustc/LLVM never materialise the ~24M kernel + ~7M agent as constants — which
//! otherwise cost significant compile time and peak memory. build.rs supplies the
//! paths via VK_EMBED_{KERNEL,AGENT}_PATH (an empty placeholder when nothing is
//! provided, giving a zero-length blob that resolves to "not embedded").
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Clone, Copy)]
pub enum Asset {
    Kernel,
    Agent,
}

impl Asset {
    /// Name used when the embedded copy is written to the scratch dir.
    fn filename(self) -> &'static str {
        match self {
            Asset::Kernel => "vmlinux",
            Asset::Agent => "vk-agent",
        }
    }

    /// On-disk fallback when nothing is embedded and no flag is given.
    fn default_path(self) -> &'static str {
        match self {
            Asset::Kernel => "/usr/local/lib/vk/vmlinux",
            Asset::Agent => "/usr/local/lib/vk/vk-agent",
        }
    }

    fn embedded(self) -> Option<&'static [u8]> {
        match self {
            Asset::Kernel => kernel(),
            Asset::Agent => agent(),
        }
    }
}

// The two blobs, spliced into `.rodata` by the linker. Each `.incbin` is bracketed
// by start/end symbols so the runtime can recover the byte range; an empty placeholder
// file (the no-asset dev build) makes start == end, i.e. a zero-length blob.
#[cfg(feature = "embed")]
mod blob {
    use std::arch::global_asm;

    global_asm!(concat!(
        ".section .rodata.vk_embed_kernel,\"a\",@progbits\n",
        ".globl VK_EMBED_KERNEL_START\n",
        "VK_EMBED_KERNEL_START:\n",
        ".incbin \"",
        env!("VK_EMBED_KERNEL_PATH"),
        "\"\n",
        ".globl VK_EMBED_KERNEL_END\n",
        "VK_EMBED_KERNEL_END:\n",
        ".section .rodata.vk_embed_agent,\"a\",@progbits\n",
        ".globl VK_EMBED_AGENT_START\n",
        "VK_EMBED_AGENT_START:\n",
        ".incbin \"",
        env!("VK_EMBED_AGENT_PATH"),
        "\"\n",
        ".globl VK_EMBED_AGENT_END\n",
        "VK_EMBED_AGENT_END:\n",
    ));

    unsafe extern "C" {
        pub static VK_EMBED_KERNEL_START: u8;
        pub static VK_EMBED_KERNEL_END: u8;
        pub static VK_EMBED_AGENT_START: u8;
        pub static VK_EMBED_AGENT_END: u8;
    }

    /// The bytes between two linker symbols (`None` if empty). The symbols bound a
    /// contiguous `.incbin` blob in `.rodata`, so the range is a single static object.
    fn between(start: *const u8, end: *const u8) -> Option<&'static [u8]> {
        let len = end as usize - start as usize;
        // SAFETY: start..end is the linker-placed .incbin blob, valid for the whole
        // program and never mutated.
        (len != 0).then(|| unsafe { std::slice::from_raw_parts(start, len) })
    }

    pub fn kernel() -> Option<&'static [u8]> {
        between(
            &raw const VK_EMBED_KERNEL_START,
            &raw const VK_EMBED_KERNEL_END,
        )
    }

    pub fn agent() -> Option<&'static [u8]> {
        between(
            &raw const VK_EMBED_AGENT_START,
            &raw const VK_EMBED_AGENT_END,
        )
    }
}

#[cfg(feature = "embed")]
fn kernel() -> Option<&'static [u8]> {
    blob::kernel()
}

#[cfg(feature = "embed")]
fn agent() -> Option<&'static [u8]> {
    blob::agent()
}

#[cfg(not(feature = "embed"))]
fn kernel() -> Option<&'static [u8]> {
    None
}

#[cfg(not(feature = "embed"))]
fn agent() -> Option<&'static [u8]> {
    None
}

/// Resolve an asset to a concrete path. An explicit override wins; otherwise the
/// embedded copy is written into `dir` (created if needed); otherwise the on-disk
/// default is used. The agent is written executable.
pub fn resolve(asset: Asset, explicit: Option<&Path>, dir: &Path) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Some(bytes) = asset.embedded() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let out = dir.join(asset.filename());
        std::fs::write(&out, bytes).with_context(|| {
            format!("writing embedded {} to {}", asset.filename(), out.display())
        })?;
        if matches!(asset, Asset::Agent) {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755))
                .with_context(|| format!("marking {} executable", out.display()))?;
        }
        return Ok(out);
    }
    Ok(PathBuf::from(asset.default_path()))
}
