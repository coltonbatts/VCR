use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// A strict sandbox that ensures all resolved paths remain within a designated root directory.
#[derive(Debug, Clone)]
pub struct ManifestSandbox {
    root: PathBuf,
}

impl ManifestSandbox {
    /// Initializes the sandbox, canonicalizing the given root directory.
    pub fn new<P: AsRef<Path>>(root: P) -> Result<Self> {
        let root = fs::canonicalize(root.as_ref()).with_context(|| {
            format!(
                "Failed to canonicalize manifest root: {}",
                root.as_ref().display()
            )
        })?;
        Ok(Self { root })
    }

    /// Safely resolves a path relative to the manifest root.
    /// Rejects any path traversing outside the root or following external symlinks.
    pub fn resolve<P: AsRef<Path>>(&self, target: P) -> Result<PathBuf> {
        let combined = self.root.join(target.as_ref());
        let canonical = fs::canonicalize(&combined).with_context(|| {
            format!("Failed to resolve or canonicalize path: {}", combined.display())
        })?;

        if !canonical.starts_with(&self.root) {
            bail!(
                "Path traversal violation: blocked access to {}",
                canonical.display()
            );
        }
        Ok(canonical)
    }

    /// Returns the canonicalized root path.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_valid_child_path() {
        let dir = tempdir().unwrap();
        let sandbox = ManifestSandbox::new(dir.path()).unwrap();

        let child_path = dir.path().join("child.txt");
        File::create(&child_path).unwrap();

        let resolved = sandbox.resolve("child.txt").unwrap();
        assert_eq!(resolved, fs::canonicalize(&child_path).unwrap());
    }

    #[test]
    fn test_path_traversal_dot_dot() {
        let dir = tempdir().unwrap();
        // We need to create it so canonicalize doesn't fail early with NotFound,
        // but wait, creating a file outside tempdir might be bad practice in tests.
        // If canonicalize fails with NotFound, that's also a rejection. 
        // Let's create a nested structure to test traversal within controlled bounds.
        
        let parent_dir = tempdir().unwrap();
        let root_dir = parent_dir.path().join("root");
        fs::create_dir(&root_dir).unwrap();
        
        let outside_file = parent_dir.path().join("outside.txt");
        File::create(&outside_file).unwrap();
        
        let sandbox = ManifestSandbox::new(&root_dir).unwrap();
        
        // Try to access outside.txt using ../
        let result = sandbox.resolve("../outside.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal violation"));
    }

    #[test]
    fn test_absolute_path_injection() {
        let parent_dir = tempdir().unwrap();
        let root_dir = parent_dir.path().join("root");
        fs::create_dir(&root_dir).unwrap();
        
        let outside_file = parent_dir.path().join("outside.txt");
        File::create(&outside_file).unwrap();
        
        let sandbox = ManifestSandbox::new(&root_dir).unwrap();
        
        // Access via absolute path
        let absolute_outside = fs::canonicalize(&outside_file).unwrap();
        let result = sandbox.resolve(absolute_outside);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal violation"));
    }
}
