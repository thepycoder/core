//! Atomic file and multi-file bundle publication.
//!
//! Single files are written to a sibling temp path and renamed into place.
//! Multi-file bundles stage every output first, then promote with backup/restore
//! so a failed publish leaves the previous canonical snapshot intact.

use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Write `bytes` to `path` via a sibling `*.tmp` file and rename.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = sibling_temp(path, "tmp")?;
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Write UTF-8 text atomically.
pub fn write_text_atomic(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    write_bytes_atomic(path, text.as_bytes())
}

fn sibling_temp(path: &Path, suffix: &str) -> Result<PathBuf, Box<dyn Error>> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or("path has no file name")?
        .to_string_lossy();
    let tmp = parent.join(format!(".{}.{}.{}", name, std::process::id(), suffix));
    Ok(tmp)
}

/// Staged multi-file publish: write to staging paths, then promote atomically.
pub struct BundlePublisher {
    label: String,
    staging_root: PathBuf,
    /// (staging path, final path)
    entries: Vec<(PathBuf, PathBuf)>,
}

impl BundlePublisher {
    pub fn new(label: &str, base_dir: &Path) -> Result<Self, Box<dyn Error>> {
        let staging_root = base_dir.join(format!(
            ".publish-{}-{}",
            sanitize_label(label),
            std::process::id()
        ));
        if staging_root.exists() {
            fs::remove_dir_all(&staging_root)?;
        }
        fs::create_dir_all(&staging_root)?;
        Ok(Self {
            label: label.to_string(),
            staging_root,
            entries: Vec::new(),
        })
    }

    pub fn staging_root(&self) -> &Path {
        &self.staging_root
    }

    /// Allocate a staging path that will be promoted to `final_path` on commit.
    pub fn stage_path(&mut self, final_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
        let rel = final_path
            .file_name()
            .ok_or("final path has no file name")?
            .to_string_lossy();
        // Keep distinct names when multiple finals share a basename (different dirs).
        let idx = self.entries.len();
        let staging = self.staging_root.join(format!("{idx}_{rel}"));
        self.entries
            .push((staging.clone(), final_path.to_path_buf()));
        Ok(staging)
    }

    /// Promote all staged files to their finals. On failure, restore prior snapshots.
    pub fn commit(self) -> Result<(), Box<dyn Error>> {
        let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
        let result = (|| {
            for (staging, final_path) in &self.entries {
                if !staging.exists() {
                    return Err(format!(
                        "bundle {}: staging file missing: {}",
                        self.label,
                        staging.display()
                    )
                    .into());
                }
                if let Some(parent) = final_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                if final_path.exists() {
                    let bak = backup_path(final_path)?;
                    fs::rename(final_path, &bak)?;
                    backups.push((bak, final_path.clone()));
                }
                fs::rename(staging, final_path)?;
            }
            Ok::<(), Box<dyn Error>>(())
        })();

        match result {
            Ok(()) => {
                for (bak, _) in backups {
                    let _ = fs::remove_file(bak);
                }
                let _ = fs::remove_dir_all(&self.staging_root);
                Ok(())
            }
            Err(err) => {
                // Restore any finals we already replaced.
                for (bak, final_path) in backups.iter().rev() {
                    let _ = fs::remove_file(final_path);
                    let _ = fs::rename(bak, final_path);
                }
                let _ = fs::remove_dir_all(&self.staging_root);
                Err(err)
            }
        }
    }

    /// Discard staging without touching finals.
    pub fn abandon(self) {
        let _ = fs::remove_dir_all(&self.staging_root);
    }
}

fn backup_path(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(format!("{}.prev", path.display())))
}

fn sanitize_label(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("crawl-atomic-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_bytes_atomic_replaces_file() {
        let dir = temp_dir();
        let path = dir.join("out.txt");
        write_bytes_atomic(&path, b"one").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"one");
        write_bytes_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bundle_commit_preserves_prior_on_missing_staging() {
        let dir = temp_dir();
        let final_a = dir.join("a.parquet");
        fs::write(&final_a, b"old-a").unwrap();

        let mut bundle = BundlePublisher::new("test", &dir).unwrap();
        let stage_a = bundle.stage_path(&final_a).unwrap();
        fs::write(&stage_a, b"new-a").unwrap();
        // Deliberately skip writing the second staged file.
        let final_b = dir.join("b.parquet");
        let _stage_b = bundle.stage_path(&final_b).unwrap();

        let err = bundle.commit().unwrap_err();
        assert!(err.to_string().contains("staging file missing"));
        assert_eq!(fs::read(&final_a).unwrap(), b"old-a");
        assert!(!final_b.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bundle_commit_promotes_all() {
        let dir = temp_dir();
        let final_a = dir.join("a.txt");
        let final_b = dir.join("sub").join("b.txt");
        fs::write(&final_a, b"old").unwrap();

        let mut bundle = BundlePublisher::new("ok", &dir).unwrap();
        let sa = bundle.stage_path(&final_a).unwrap();
        let sb = bundle.stage_path(&final_b).unwrap();
        fs::write(&sa, b"new-a").unwrap();
        fs::write(&sb, b"new-b").unwrap();
        bundle.commit().unwrap();

        assert_eq!(fs::read(&final_a).unwrap(), b"new-a");
        assert_eq!(fs::read(&final_b).unwrap(), b"new-b");
        fs::remove_dir_all(&dir).unwrap();
    }
}
