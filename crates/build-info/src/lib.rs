//! One compiled identity for the desktop and its embedded/standalone MCP.
#[derive(Clone, Debug, serde::Serialize)]
pub struct BuildInfo {
    pub version: &'static str,
    pub revision: &'static str,
    pub channel: &'static str,
    pub modified: bool,
}

/// Owned GitHub repository for opt-in hosted diagnostics and their provenance.
pub fn repository_slug() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../REPOSITORY")).trim()
}

/// Stable repository identity for hosted diagnostics across an ownership move.
pub fn repository_id() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../REPOSITORY_ID")).trim()
}

pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        revision: env!("LIMO_CAD_BUILD_REVISION"),
        channel: env!("LIMO_CAD_BUILD_CHANNEL"),
        modified: env!("LIMO_CAD_BUILD_MODIFIED") == "true",
    }
}

impl BuildInfo {
    pub fn display_version(&self) -> String {
        format!(
            "{}+{}{}",
            self.version,
            self.revision,
            if self.modified { ".modified" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_version_keeps_the_full_revision_and_modified_marker() {
        let mut info = BuildInfo {
            version: "0.2.0",
            revision: "abcdef0123456789abcdef0123456789abcdef01",
            channel: "preview",
            modified: false,
        };
        assert_eq!(info.display_version(), format!("0.2.0+{}", info.revision));
        info.modified = true;
        assert_eq!(
            info.display_version(),
            format!("0.2.0+{}.modified", info.revision)
        );
    }
}

#[cfg(test)]
mod version_carriers {
    /// `BuildInfo::version` is the product version the desktop About dialog and
    /// the MCP `initialize` result report, so the manifest that carries it must
    /// agree with the repository's product version. This crate was split out of
    /// the engine workspace to keep source identity from invalidating the CAD
    /// dependency graph. VERSION must agree with the compiled package identity.
    #[test]
    fn packaged_version_matches_the_product_version() {
        let declared = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../VERSION")).trim();
        assert_eq!(
            env!("CARGO_PKG_VERSION"),
            declared,
            "limo-cad-build-info must carry the product version; keep crates/build-info/Cargo.toml \
             in step with VERSION"
        );
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod build_script_tests {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"));
}
