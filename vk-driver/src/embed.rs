//! The guest kernel and vk-agent, optionally embedded into the `vk` binary.
//!
//! The shipped `vk` is self-contained: build.sh compiles it with the `embed`
//! feature and points build.rs at a freshly built vk-agent and the pinned
//! vmlinux, which land here as `include_bytes!`. At runtime an asset is resolved
//! in this order: an explicit --kernel/--agent path, else the embedded copy
//! (written into the launch's scratch dir), else the on-disk default under
//! /usr/local/lib/vk. A plain dev `cargo build` embeds nothing, so it just uses
//! the flags or the on-disk defaults.
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

#[cfg(feature = "embed")]
fn kernel() -> Option<&'static [u8]> {
    let b: &[u8] = include_bytes!(env!("VK_EMBED_KERNEL_PATH"));
    (!b.is_empty()).then_some(b)
}

#[cfg(feature = "embed")]
fn agent() -> Option<&'static [u8]> {
    let b: &[u8] = include_bytes!(env!("VK_EMBED_AGENT_PATH"));
    (!b.is_empty()).then_some(b)
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
