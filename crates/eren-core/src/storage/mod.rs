//! Object storage for knowledge-base assets — MinIO, or anything speaking S3.
//!
//! Article *text* lives in Postgres; only the things people paste into an
//! editor (images, PDFs, zips) come here. That split matters: text needs to be
//! searchable and transactional with the row that owns it, while a 4 MB
//! screenshot needs neither and would bloat every backup.
//!
//! Configuration is explicit, from `EREN_S3_*`. Nothing is discovered: no
//! `~/.aws`, no environment credential chain, no instance metadata. If it
//! isn't configured, [`Storage::from_env`] returns `None` and uploads are
//! refused with a message saying so — the knowledge base still works, you just
//! can't attach files to it yet.

pub mod sigv4;

use sigv4::{Credentials, Request};

/// A configured object store. Cheap to clone; holds no connection of its own.
#[derive(Clone)]
pub struct Storage {
    client: reqwest::Client,
    /// e.g. `http://127.0.0.1:9000`, no trailing slash.
    endpoint: String,
    bucket: String,
    /// Whether somebody named the bucket. If not, the default may still be
    /// the one from before the rename (see [`Storage::ensure_bucket`]).
    bucket_chosen: bool,
    region: String,
    access_key: String,
    secret_key: String,
}

/// Biggest single upload. Generous for a screenshot or a PDF, small enough
/// that one paste can't fill a disk.
pub const MAX_OBJECT_BYTES: usize = 25 * 1024 * 1024;

impl Storage {
    /// Read the configuration, or `None` when storage isn't set up.
    ///
    /// Absent configuration is a normal state, not an error: a fresh install
    /// has no MinIO, and the knowledge base is still useful without one.
    pub fn from_env() -> Option<Self> {
        let endpoint = eren_shared::brand::var("S3_ENDPOINT")?;
        let access_key = eren_shared::brand::var("S3_ACCESS_KEY")?;
        let secret_key = eren_shared::brand::var("S3_SECRET_KEY")?;
        let bucket = eren_shared::brand::var("S3_BUCKET");
        Some(Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            bucket_chosen: bucket.is_some(),
            bucket: bucket.unwrap_or_else(|| eren_shared::brand::BUCKET.into()),
            region: eren_shared::brand::var("S3_REGION").unwrap_or_else(|| "us-east-1".into()),
            access_key,
            secret_key,
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    fn host(&self) -> &str {
        self.endpoint
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&self.endpoint)
    }

    /// Path-style addressing (`/bucket/key`) rather than virtual-host style.
    /// MinIO on a bare IP has no wildcard DNS, so `bucket.127.0.0.1` does not
    /// resolve; path style is the only shape that works everywhere.
    fn object_path(&self, key: &str) -> String {
        let encoded: Vec<String> = key.split('/').map(sigv4::encode_segment).collect();
        format!(
            "/{}/{}",
            sigv4::encode_segment(&self.bucket),
            encoded.join("/")
        )
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        query: &str,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> anyhow::Result<reqwest::Response> {
        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let signed = sigv4::sign(
            &Request {
                method,
                path,
                query,
                host: self.host(),
                payload: &body,
                timestamp: &timestamp,
            },
            &Credentials {
                access_key: &self.access_key,
                secret_key: &self.secret_key,
                region: &self.region,
            },
        );

        let url = if query.is_empty() {
            format!("{}{}", self.endpoint, path)
        } else {
            format!("{}{}?{}", self.endpoint, path, query)
        };
        let mut req = self
            .client
            .request(reqwest::Method::from_bytes(method.as_bytes())?, url);
        for (k, v) in signed {
            req = req.header(k, v);
        }
        // Set after signing, deliberately: `content-type` is not a signed
        // header here, so adding it to the signature would make every request
        // fail with a mismatch.
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        Ok(req.body(body).send().await?)
    }

    /// Create the bucket if it isn't there. Idempotent — an existing bucket
    /// reports `BucketAlreadyOwnedByYou`, which is success, not a clash.
    ///
    /// With no bucket named, the default changed with the project's name, and
    /// every attachment uploaded before that is in the old one. So when the
    /// new default does not exist yet and the old one does, the old one is
    /// used: creating an empty bucket beside it would make every existing
    /// image in the knowledge base a broken link.
    pub async fn ensure_bucket(&mut self) -> anyhow::Result<()> {
        if !self.bucket_chosen {
            let legacy = eren_shared::brand::LEGACY_BUCKET;
            let current = self.bucket_exists(&self.bucket).await?;
            if adopt_legacy_bucket(current, self.bucket_exists(legacy).await?) {
                self.bucket = legacy.to_string();
                return Ok(());
            }
        }
        let path = format!("/{}", sigv4::encode_segment(&self.bucket));
        let res = self.send("PUT", &path, "", vec![], None).await?;
        let status = res.status();
        if status.is_success() || status == reqwest::StatusCode::CONFLICT {
            return Ok(());
        }
        let body = res.text().await.unwrap_or_default();
        if body.contains("BucketAlreadyOwnedByYou") || body.contains("BucketAlreadyExists") {
            return Ok(());
        }
        anyhow::bail!("could not create bucket {}: {status} {body}", self.bucket);
    }

    async fn bucket_exists(&self, name: &str) -> anyhow::Result<bool> {
        let path = format!("/{}", sigv4::encode_segment(name));
        let res = self.send("HEAD", &path, "", vec![], None).await?;
        Ok(res.status().is_success())
    }

    pub async fn put(&self, key: &str, body: Vec<u8>, content_type: &str) -> anyhow::Result<()> {
        if body.len() > MAX_OBJECT_BYTES {
            anyhow::bail!(
                "that file is {:.1} MB; the limit is {} MB",
                body.len() as f64 / 1_048_576.0,
                MAX_OBJECT_BYTES / 1_048_576
            );
        }
        let path = self.object_path(key);
        let res = self
            .send("PUT", &path, "", body, Some(content_type))
            .await?;
        if !res.status().is_success() {
            let status = res.status();
            anyhow::bail!(
                "upload failed: {status} {}",
                res.text().await.unwrap_or_default()
            );
        }
        Ok(())
    }

    /// Fetch an object. `None` when it isn't there, which callers turn into a
    /// 404 rather than a 500 — a deleted asset is a missing page, not a fault.
    pub async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let path = self.object_path(key);
        let res = self.send("GET", &path, "", vec![], None).await?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !res.status().is_success() {
            let status = res.status();
            anyhow::bail!(
                "fetch failed: {status} {}",
                res.text().await.unwrap_or_default()
            );
        }
        Ok(Some(res.bytes().await?.to_vec()))
    }

    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let path = self.object_path(key);
        let res = self.send("DELETE", &path, "", vec![], None).await?;
        // 204 on success, 404 if it was already gone — both mean "not there
        // any more", which is what the caller asked for.
        if res.status().is_success() || res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        let status = res.status();
        anyhow::bail!(
            "delete failed: {status} {}",
            res.text().await.unwrap_or_default()
        );
    }
}

/// Where an asset lives in the bucket.
///
/// Keyed by asset id rather than by filename: two people uploading
/// `screenshot.png` must not collide, and a filename is user input that has no
/// business becoming a path.
pub fn object_key(asset_id: uuid::Uuid, filename: &str) -> String {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();
    format!("kb/{asset_id}{ext}")
}

/// Whether to keep using the bucket from before the rename: only when nobody
/// named one, the new default has never been made, and the old one exists.
fn adopt_legacy_bucket(current_exists: bool, legacy_exists: bool) -> bool {
    !current_exists && legacy_exists
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_old_bucket_is_kept_only_while_it_is_the_only_one() {
        use super::adopt_legacy_bucket;
        assert!(adopt_legacy_bucket(false, true));
        // Once the new one exists it wins — it is where uploads have gone.
        assert!(!adopt_legacy_bucket(true, true));
        assert!(!adopt_legacy_bucket(true, false));
        // A fresh install makes the new one.
        assert!(!adopt_legacy_bucket(false, false));
    }

    use super::*;

    #[test]
    fn a_filename_cannot_escape_its_prefix() {
        // The filename is user input. Only its extension survives, so no
        // amount of `../` in it can move the object.
        let id = uuid::Uuid::nil();
        let key = object_key(id, "../../etc/passwd");
        assert!(key.starts_with("kb/"), "{key}");
        assert!(!key.contains(".."), "{key}");

        let key = object_key(id, "a/b/c.png");
        assert_eq!(key, format!("kb/{id}.png"));
    }

    #[test]
    fn a_hostile_extension_is_dropped_rather_than_trusted() {
        let id = uuid::Uuid::nil();
        // Too long, or not alphanumeric: no extension at all beats a crafted
        // one, since the stored key never decides how bytes are served.
        assert_eq!(object_key(id, "x.thisisaverylongext"), format!("kb/{id}"));
        assert_eq!(object_key(id, "x.p n g"), format!("kb/{id}"));
        assert_eq!(object_key(id, "noextension"), format!("kb/{id}"));
    }

    #[test]
    fn extensions_are_normalised() {
        let id = uuid::Uuid::nil();
        assert_eq!(object_key(id, "SHOT.PNG"), format!("kb/{id}.png"));
    }
}
