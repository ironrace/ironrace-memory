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

    /// Recursively list all blob (file) paths under the commit's tree.
    /// Used by R7 (rename candidate) for whole-tree similarity search.
    pub fn list_tree(&self, commit_sha: &str) -> Result<Vec<String>> {
        let oid = ObjectId::from_hex(commit_sha.as_bytes())
            .with_context(|| format!("parsing commit sha {}", commit_sha))?;
        let commit = match self.inner.find_object(oid) {
            Ok(o) => o.try_into_commit().context("not a commit")?,
            Err(_) => return Ok(Vec::new()),
        };
        let tree = commit.tree().context("commit has no tree")?;
        let mut out = Vec::new();
        walk_tree(&self.inner, &tree, "", &mut out)?;
        out.sort();
        Ok(out)
    }
}

fn walk_tree(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: &str,
    out: &mut Vec<String>,
) -> Result<()> {
    for entry in tree.iter() {
        let entry = entry?;
        let name = entry.filename().to_string();
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        let mode = entry.mode();
        if mode.is_tree() {
            let child = repo.find_object(entry.object_id())?;
            let child_tree = child.try_into_tree().context("expected tree object")?;
            walk_tree(repo, &child_tree, &full, out)?;
        } else if mode.is_blob() {
            out.push(full);
        }
    }
    Ok(())
}
