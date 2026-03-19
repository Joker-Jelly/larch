use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Marker file inside .larch/ to identify a valid vault.
const VAULT_MARKER: &str = "vault.json";

/// Global config file name.
const GLOBAL_CONFIG_FILE: &str = "config.json";

/// Default vault directory name (under home).
const DEFAULT_VAULT_DIR: &str = ".larch-vault";

// ── Global config (lives in ~/.larch/) ──────────────────────────────

/// Global configuration stored in `~/.larch/config.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GlobalConfig {
    pub vault_path: PathBuf,
}

/// Returns the global config directory: `~/.larch/`
pub fn global_config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home.join(".larch"))
}

/// Returns the path to `~/.larch/config.json`.
pub fn global_config_path() -> Result<PathBuf> {
    Ok(global_config_dir()?.join(GLOBAL_CONFIG_FILE))
}

/// Default vault directory: `~/.larch-vault/`
pub fn default_vault_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home.join(DEFAULT_VAULT_DIR))
}

/// Load the global config from `~/.larch/config.json`.
pub fn load_global_config() -> Result<GlobalConfig> {
    let path = global_config_path()?;
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("reading global config: {}", path.display()))?;
    let config: GlobalConfig = serde_json::from_str(&data)
        .with_context(|| format!("parsing global config: {}", path.display()))?;
    Ok(config)
}

/// Save the global config to `~/.larch/config.json`.
pub fn save_global_config(config: &GlobalConfig) -> Result<()> {
    let dir = global_config_dir()?;
    std::fs::create_dir_all(&dir)
        .context("creating global config dir")?;
    let path = dir.join(GLOBAL_CONFIG_FILE);
    let data = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, data)
        .with_context(|| format!("writing global config: {}", path.display()))?;
    Ok(())
}

/// Resolve the vault directory.
///
/// Priority:
/// 1. `~/.larch/config.json` → `vault_path`
/// 2. Migration: if `~/.larch/vault.json` exists (old layout where ~/.larch was
///    the vault itself), create config.json pointing to `~/.larch/` for backward compat
/// 3. Error: no vault configured
pub fn get_vault_dir() -> Result<PathBuf> {
    let config_path = global_config_path()?;
    if config_path.exists() {
        let gc = load_global_config()?;
        return Ok(gc.vault_path);
    }

    // Migration: old layout had vault directly at ~/.larch/
    // The vault marker would be at ~/.larch/.larch/vault.json
    let config_dir = global_config_dir()?;
    let old_marker = config_dir.join(".larch").join(VAULT_MARKER);
    if old_marker.exists() {
        // The old vault IS ~/.larch/ — write config.json pointing to it
        let gc = GlobalConfig {
            vault_path: config_dir.clone(),
        };
        save_global_config(&gc)?;
        return Ok(config_dir);
    }

    anyhow::bail!(
        "No vault configured. Run `larch init` or `larch init <path>` first."
    )
}

// ── Vault config ────────────────────────────────────────────────────

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

    pub fn serve_lock_path(&self) -> std::path::PathBuf {
        self.larch_dir().join("serve.lock")
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
