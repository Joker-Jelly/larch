use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub struct ServeClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
pub struct ImportFileResponse {
    pub success: bool,
    pub files_imported: usize,
}

#[derive(Deserialize)]
pub struct ReindexResponse {
    pub success: bool,
    pub files_indexed: usize,
}

#[derive(Serialize)]
struct ImportFileRequest {
    source_path: String,
    move_file: bool,
    dir: Option<String>,
}

impl ServeClient {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{}", port),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("building HTTP client"),
        }
    }

    /// Check if the serve process is reachable.
    pub async fn is_healthy(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Delegate file import to serve.
    pub async fn import_file(
        &self,
        source_path: &str,
        move_file: bool,
        dir: Option<&str>,
    ) -> Result<ImportFileResponse> {
        let body = ImportFileRequest {
            source_path: source_path.to_string(),
            move_file,
            dir: dir.map(|s| s.to_string()),
        };
        let resp = self
            .client
            .post(format!("{}/api/v1/import/file", self.base_url))
            .json(&body)
            .send()
            .await
            .context("connecting to larch serve")?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Server returned error: {}", text);
        }
        resp.json().await.context("parsing import response")
    }

    /// Delegate reindex to serve.
    pub async fn reindex(&self) -> Result<ReindexResponse> {
        let resp = self
            .client
            .post(format!("{}/api/v1/reindex", self.base_url))
            .send()
            .await
            .context("connecting to larch serve")?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Server returned error: {}", text);
        }
        resp.json().await.context("parsing reindex response")
    }
}
