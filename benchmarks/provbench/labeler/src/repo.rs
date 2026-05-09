//! Pilot repo handling: open an already-cloned repo and walk first-parent
//! commits from T₀ forward. Cloning itself is left to a CLI step (or to
//! the user) — the labeler refuses to mutate git state.

use anyhow::{anyhow, Context, Result};
use gix::ObjectId;
use std::path::{Path, PathBuf};

/// Pinned pilot repo per SPEC §13.2.
pub const RIPGREP_T0_SHA: &str = "af6b6c543b224d348a8876f0c06245d9ea7929c5";
pub const RIPGREP_URL: &str = "https://github.com/BurntSushi/ripgrep";

pub trait PilotRepoSpec {
    fn local_clone_path(&self) -> &Path;
    fn t0_sha(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct Ripgrep {
    pub clone_path: PathBuf,
}

impl PilotRepoSpec for Ripgrep {
    fn local_clone_path(&self) -> &Path {
        &self.clone_path
    }
    fn t0_sha(&self) -> &str {
        RIPGREP_T0_SHA
    }
}

#[derive(Debug, Clone)]
pub struct CommitRef {
    pub sha: String,
    pub parent_sha: Option<String>,
}

pub struct Pilot {
    repo: gix::Repository,
    t0_sha: ObjectId,
}

impl Pilot {
    pub fn open<S: PilotRepoSpec>(spec: &S) -> Result<Self> {
        let repo = gix::open(spec.local_clone_path())
            .with_context(|| format!("open pilot repo at {}", spec.local_clone_path().display()))?;
        let t0_sha = ObjectId::from_hex(spec.t0_sha().as_bytes())
            .with_context(|| format!("parse t0 sha {}", spec.t0_sha()))?;
        repo.find_object(t0_sha)
            .with_context(|| format!("t0 commit {} not present in clone", spec.t0_sha()))?;
        Ok(Self { repo, t0_sha })
    }

    /// Walk first-parent linear history from T₀ forward up to HEAD.
    /// Returns commits in chronological order (oldest first).
    pub fn walk_first_parent(&self) -> Result<impl Iterator<Item = CommitRef> + '_> {
        let head = self.repo.head_commit().context("resolve HEAD")?;
        // Store (commit_id, first_parent_id) so we don't re-read from the ODB
        // in the output map and can avoid any silent error swallowing.
        let mut chain: Vec<(ObjectId, Option<ObjectId>)> = Vec::new();
        // head.id is the public ObjectId field on Commit<'_>
        let mut cur = Some(head.id);
        while let Some(id) = cur {
            let commit = self
                .repo
                .find_commit(id)
                .with_context(|| format!("walk: find {id}"))?;
            // parent_ids() yields gix::Id<'_>; detach() extracts the inner ObjectId
            let parent_id = commit.parent_ids().next().map(|p| p.detach());
            chain.push((id, parent_id));
            if id == self.t0_sha {
                break;
            }
            cur = parent_id;
        }
        if chain.last().map(|(id, _)| id) != Some(&self.t0_sha) {
            return Err(anyhow!(
                "first-parent chain from HEAD does not contain T₀ {}; rebased history?",
                self.t0_sha
            ));
        }
        chain.reverse();
        Ok(chain.into_iter().map(|(id, parent)| CommitRef {
            sha: id.to_string(),
            parent_sha: parent.map(|p| p.to_string()),
        }))
    }

    pub fn read_blob_at(&self, commit_sha: &str, path: &Path) -> Result<Option<Vec<u8>>> {
        let id = ObjectId::from_hex(commit_sha.as_bytes())
            .with_context(|| format!("read_blob_at: invalid sha {commit_sha}"))?;
        let commit = self
            .repo
            .find_commit(id)
            .with_context(|| format!("read_blob_at: commit {commit_sha} not in repo"))?;
        let tree = commit
            .tree()
            .with_context(|| format!("read_blob_at: tree for {commit_sha}"))?;
        let entry = tree
            .lookup_entry_by_path(path, &mut Vec::new())
            .with_context(|| format!("read_blob_at: lookup {} @ {commit_sha}", path.display()))?;
        match entry {
            None => Ok(None),
            Some(e) => {
                let obj = e.object().with_context(|| {
                    format!(
                        "read_blob_at: load object for {} @ {commit_sha}",
                        path.display()
                    )
                })?;
                Ok(Some(obj.data.clone()))
            }
        }
    }
}
