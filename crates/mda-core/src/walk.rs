//! Find the markdown files under a watched root.
//!
//! [`discover`] is git-aware: it honours `.gitignore` (whether or not the root is a git
//! repository), `.ignore`, `.git/info/exclude`, a `.markdownattractorignore` file with the same
//! syntax, and the `ignore` patterns from [`Config`]. Hidden entries (dotfiles and
//! dot-directories) are skipped, which already covers `.git/` and `.markdownattractor/`, but
//! both are excluded explicitly as well so that no ignore file can whitelist them back in.
//! Symlinks are never followed, so a loop cannot hang the walk.
//!
//! The result is absolute paths in lexical order, so two runs over the same tree produce the
//! same list and the same work order.

use std::path::{Path, PathBuf};

use ignore::overrides::OverrideBuilder;
use ignore::{DirEntry, WalkBuilder};

use crate::config::{Config, STATE_DIR};
use crate::{Error, Result};

/// Name of the project-local ignore file, gitignore syntax, read from the root and any
/// subdirectory.
pub const IGNORE_FILE: &str = ".markdownattractorignore";

/// Extensions (lower-cased, without the dot) that count as markdown.
const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Whether `path` has a markdown extension (`.md` or `.markdown`, any case).
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MARKDOWN_EXTENSIONS.iter().any(|m| e.eq_ignore_ascii_case(m)))
}

/// All markdown files under `root`, as absolute paths in lexical order.
///
/// Fails with [`Error::Walk`] when an ignore file or a `config.ignore` pattern cannot be
/// parsed. Entries that cannot be read (permissions, vanished files) are skipped with a
/// warning.
pub fn discover(root: &Path, config: &Config) -> Result<Vec<PathBuf>> {
    let root = std::path::absolute(root).map_err(|e| Error::io(root, e))?;

    let mut overrides = OverrideBuilder::new(&root);
    // In an override set, a leading `!` means "ignore"; plain globs whitelist.
    for pattern in config.ignore.iter().chain([&format!("{STATE_DIR}/"), &".git/".to_owned()]) {
        let glob = match pattern.strip_prefix('!') {
            Some(negated) => negated.to_owned(),
            None => format!("!{pattern}"),
        };
        overrides.add(&glob)?;
    }
    let overrides = overrides.build()?;

    let mut builder = WalkBuilder::new(&root);
    builder
        .follow_links(false)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(false)
        .require_git(false)
        .ignore(true)
        .parents(false)
        .add_custom_ignore_filename(IGNORE_FILE)
        .overrides(overrides)
        .sort_by_file_path(Ord::cmp);

    let mut found = Vec::new();
    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(%err, "skipping unreadable entry");
                continue;
            }
        };
        if let Some(err) = entry.error() {
            if err.is_io() {
                tracing::warn!(path = %entry.path().display(), %err, "skipping unreadable entry");
            } else {
                return Err(Error::Walk(clone_ignore_error(err)));
            }
        }
        if is_regular_file(&entry) && is_markdown(entry.path()) {
            found.push(entry.into_path());
        }
    }
    found.sort();
    tracing::debug!(root = %root.display(), files = found.len(), "discovered markdown files");
    Ok(found)
}

/// True for plain files. Symlinks report their own type because links are not followed,
/// so they fall out here.
fn is_regular_file(entry: &DirEntry) -> bool {
    entry.file_type().is_some_and(|t| t.is_file())
}

/// `ignore::Error` is not `Clone`, and the walker only lends parse errors by reference.
fn clone_ignore_error(err: &ignore::Error) -> ignore::Error {
    use ignore::Error as E;
    match err {
        E::Partial(errs) => E::Partial(errs.iter().map(clone_ignore_error).collect()),
        E::WithLineNumber { line, err } => {
            E::WithLineNumber { line: *line, err: Box::new(clone_ignore_error(err)) }
        }
        E::WithPath { path, err } => {
            E::WithPath { path: path.clone(), err: Box::new(clone_ignore_error(err)) }
        }
        E::WithDepth { depth, err } => {
            E::WithDepth { depth: *depth, err: Box::new(clone_ignore_error(err)) }
        }
        E::Loop { ancestor, child } => E::Loop { ancestor: ancestor.clone(), child: child.clone() },
        E::Io(io) => E::Io(std::io::Error::new(io.kind(), io.to_string())),
        E::Glob { glob, err } => E::Glob { glob: glob.clone(), err: err.clone() },
        E::UnrecognizedFileType(t) => E::UnrecognizedFileType(t.clone()),
        E::InvalidDefinition => E::InvalidDefinition,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn rel_list(root: &Path, config: &Config) -> Vec<String> {
        let abs_root = std::path::absolute(root).unwrap();
        discover(root, config)
            .unwrap()
            .into_iter()
            .map(|p| {
                assert!(p.is_absolute());
                p.strip_prefix(&abs_root).unwrap().to_string_lossy().replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn is_markdown_accepts_md_and_markdown_any_case() {
        assert!(is_markdown(Path::new("a.md")));
        assert!(is_markdown(Path::new("a.MD")));
        assert!(is_markdown(Path::new("dir/b.markdown")));
        assert!(!is_markdown(Path::new("a.mdx")));
        assert!(!is_markdown(Path::new("a.txt")));
        assert!(!is_markdown(Path::new("md")));
        assert!(!is_markdown(Path::new("README")));
    }

    #[test]
    fn finds_markdown_in_nested_dirs_and_skips_other_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "top.md", "# t");
        write(dir.path(), "a/one.markdown", "# 1");
        write(dir.path(), "a/b/c/deep.md", "# d");
        write(dir.path(), "a/notes.txt", "no");
        write(dir.path(), "a/b/image.png", "no");
        write(dir.path(), "a/b/page.mdx", "no");
        let found = rel_list(dir.path(), &Config::default());
        assert_eq!(found, vec!["a/b/c/deep.md", "a/one.markdown", "top.md"]);
    }

    #[test]
    fn order_is_deterministic_and_sorted() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["z.md", "m/b.md", "m/a.md", "a.md", "m/c/x.md"] {
            write(dir.path(), name, "#");
        }
        let first = rel_list(dir.path(), &Config::default());
        let mut sorted = first.clone();
        sorted.sort();
        assert_eq!(first, sorted);
        for _ in 0..3 {
            assert_eq!(rel_list(dir.path(), &Config::default()), first);
        }
    }

    #[test]
    fn honours_gitignore_without_a_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".gitignore", "build/\n*.draft.md\n");
        write(dir.path(), "keep.md", "#");
        write(dir.path(), "build/out.md", "#");
        write(dir.path(), "notes.draft.md", "#");
        write(dir.path(), "sub/.gitignore", "secret.md\n");
        write(dir.path(), "sub/secret.md", "#");
        write(dir.path(), "sub/public.md", "#");
        assert_eq!(rel_list(dir.path(), &Config::default()), vec!["keep.md", "sub/public.md"]);
    }

    #[test]
    fn honours_custom_ignore_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), IGNORE_FILE, "archive/\nscratch*.md\n");
        write(dir.path(), "keep.md", "#");
        write(dir.path(), "archive/old.md", "#");
        write(dir.path(), "scratch-1.md", "#");
        assert_eq!(rel_list(dir.path(), &Config::default()), vec!["keep.md"]);
    }

    #[test]
    fn honours_config_ignore_patterns() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "keep.md", "#");
        write(dir.path(), "node_modules/pkg/README.md", "#");
        write(dir.path(), "vendor/x.md", "#");
        write(dir.path(), "gen/x.md", "#");
        write(dir.path(), "big.min.md", "#");
        write(dir.path(), "docs/private.md", "#");
        let mut config = Config::default();
        config.ignore.push("gen/".into());
        config.ignore.push("**/private.md".into());
        assert_eq!(rel_list(dir.path(), &config), vec!["keep.md"]);
    }

    #[test]
    fn config_ignore_can_be_emptied() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "vendor/x.md", "#");
        let config = Config { ignore: vec![], ..Config::default() };
        assert_eq!(rel_list(dir.path(), &config), vec!["vendor/x.md"]);
    }

    #[test]
    fn state_dir_and_git_dir_are_always_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "keep.md", "#");
        write(dir.path(), &format!("{STATE_DIR}/notes.md"), "#");
        write(dir.path(), ".git/README.md", "#");
        write(dir.path(), ".hidden/h.md", "#");
        // Neither config nor an ignore file that tries to whitelist them changes that.
        write(dir.path(), ".gitignore", "!.markdownattractor/\n!.git/\n");
        let config = Config { ignore: vec![], ..Config::default() };
        assert_eq!(rel_list(dir.path(), &config), vec!["keep.md"]);
    }

    #[test]
    fn malformed_ignore_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "keep.md", "#");
        write(dir.path(), IGNORE_FILE, "ok/\n[z-a]\n");
        let err = discover(dir.path(), &Config::default()).unwrap_err();
        assert!(matches!(err, Error::Walk(_)), "{err}");
    }

    #[test]
    fn malformed_config_pattern_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config { ignore: vec!["[z-a]".into()], ..Config::default() };
        let err = discover(dir.path(), &config).unwrap_err();
        assert!(matches!(err, Error::Walk(_)), "{err}");
    }

    #[test]
    fn empty_root_yields_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover(dir.path(), &Config::default()).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loop_does_not_hang_and_links_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a/real.md", "#");
        std::os::unix::fs::symlink(dir.path().join("a"), dir.path().join("a/loop")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("a/real.md"), dir.path().join("link.md"))
            .unwrap();
        assert_eq!(rel_list(dir.path(), &Config::default()), vec!["a/real.md"]);
    }
}
