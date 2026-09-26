use embed_manifest::manifest::{ExecutionLevel, HeapType};
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        // Admin is required to talk to the PawnIO driver (CPU temperature).
        // Default manifest already enables per-monitor DPI v2 and common controls v6.
        embed_manifest(
            new_manifest("SysWidget")
                .requested_execution_level(ExecutionLevel::RequireAdministrator)
                .heap_type(HeapType::SegmentHeap),
        )
        .expect("unable to embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
