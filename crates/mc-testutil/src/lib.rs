//! Shared inputs for tests that need golden dumps (`tools/extract/extract.sh`) or the vanilla jar.
//! A missing input skips the test, unless `RUSTMC_REQUIRE_GOLDEN=1` is set, which makes it fail.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Returns `None` (after logging) when `what` is missing; panics instead under `RUSTMC_REQUIRE_GOLDEN=1`.
pub fn require(what: &Path, found: bool) -> Option<PathBuf> {
    if found {
        return Some(what.to_path_buf());
    }
    let msg = format!("{} not found (tools/extract/extract.sh, or set RUSTMC_MCP)", what.display());
    if std::env::var("RUSTMC_REQUIRE_GOLDEN").is_ok_and(|v| v == "1") {
        panic!("RUSTMC_REQUIRE_GOLDEN=1 but {msg}");
    }
    eprintln!("skipping: {msg}");
    None
}

/// `<repo>/golden/<name>`.
pub fn golden(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os("RUSTMC_GOLDEN_DIR").map(PathBuf::from).unwrap_or_else(|| root().join("golden")).join(name);
    require(&p, p.is_file())
}

/// The vanilla 1.8.9 jar under `$RUSTMC_MCP`, `<repo>/MCP-919` or `<repo>/../MCP-919`.
pub fn jar() -> Option<PathBuf> {
    let rel = "jars/versions/1.8.9/1.8.9.jar";
    let mut dirs = vec![];
    if let Ok(d) = std::env::var("RUSTMC_MCP") {
        dirs.push(PathBuf::from(d));
    }
    dirs.push(root().join("MCP-919"));
    dirs.push(root().join("../MCP-919"));
    let hit = dirs.iter().map(|d| d.join(rel)).find(|p| p.is_file());
    let p = hit.unwrap_or_else(|| dirs[0].join(rel));
    require(&p, p.is_file())
}
