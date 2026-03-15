use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Marker file inside .larch/ to identify a valid vault.
const VAULT_MARKER: &str = "vault.json";

/// Central configuration for a Larch vault.
#[derive(Debug, Clone)]
pub struct VaultConfig {
    pub vault_root: PathBuf,
}

impl VaultConfig {
    // ── directory helpers ────────────────────────────────────────────

    pub fn larch_dir(&self) -> PathBuf {
        self.vault_root.join(".larch")
    }

    pub fn index_dir(&self) -> PathBuf {
        self.larch_dir().join("index")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.larch_dir().join("logs")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.vault_root.join("assets")
    }

    fn marker_path(&self) -> PathBuf {
        self.larch_dir().join(VAULT_MARKER)
    }

    // ── lifecycle ───────────────────────────────────────────────────

    /// Initialise a brand-new vault (directories + marker).
    /// Returns an error if the vault is already initialised.
    pub fn init(dir: &Path) -> Result<Self> {
        let vault_root = std::fs::canonicalize(dir)
            .unwrap_or_else(|_| dir.to_path_buf());

        let config = Self {
            vault_root: vault_root.clone(),
        };

        if config.marker_path().exists() {
            anyhow::bail!("Vault already initialised at {}", vault_root.display());
        }

        // Create directory tree
        std::fs::create_dir_all(&vault_root)
            .context("creating vault root")?;
        std::fs::create_dir_all(config.larch_dir())
            .context("creating .larch dir")?;
        std::fs::create_dir_all(config.index_dir())
            .context("creating index dir")?;
        std::fs::create_dir_all(config.logs_dir())
            .context("creating logs dir")?;
        std::fs::create_dir_all(config.assets_dir())
            .context("creating assets dir")?;

        // Write a minimal marker / config file
        let marker = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "created_at": chrono::Utc::now().to_rfc3339(),
        });
        std::fs::write(config.marker_path(), serde_json::to_string_pretty(&marker)?)
            .context("writing vault marker")?;

        Ok(config)
    }

    /// Open an existing vault (validates marker).
    pub fn open(dir: &Path) -> Result<Self> {
        let vault_root = std::fs::canonicalize(dir)
            .with_context(|| format!("vault path not found: {}", dir.display()))?;
        let config = Self { vault_root };
        if !config.marker_path().exists() {
            anyhow::bail!(
                "Not a Larch vault (missing .larch/{}). Run `larch init` first.",
                VAULT_MARKER
            );
        }
        Ok(config)
    }
}
