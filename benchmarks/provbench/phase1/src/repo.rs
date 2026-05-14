//! Single-repo HEAD-only reader: open the repo, read a blob at a commit,
//! check file existence at a commit.

use anyhow::{Context, Result};
use gix::ObjectId;
use std::path::Path;

pub struct Repo {
    inner: gix::Repository,
}

impl Repo {
    pub fn open(path: &Path) -> Result<Self> {
        let inner = gix::open(path).with_context(|| format!("opening repo {}", path.display()))?;
        Ok(Self { inner })
    }

    pub fn file_exists_at(&self, commit_sha: &str, source_path: &str) -> Result<bool> {
        Ok(self.blob_at(commit_sha, source_path)?.is_some())
    }

    pub fn blob_at(&self, commit_sha: &str, source_path: &str) -> Result<Option<Vec<u8>>> {
        let oid = ObjectId::from_hex(commit_sha.as_bytes())
            .with_context(|| format!("parsing commit sha {}", commit_sha))?;
        let commit = match self.inner.find_object(oid) {
            Ok(o) => o.try_into_commit().context("not a commit")?,
            Err(_) => return Ok(None),
        };
        let tree = commit.tree().context("commit has no tree")?;
        let mut buf = Vec::new();
        let entry = match tree.lookup_entry_by_path(source_path, &mut buf)? {
            Some(e) => e,
            None => return Ok(None),
        };
        let obj = entry.object()?;
        Ok(Some(obj.data.clone()))
    }
}
