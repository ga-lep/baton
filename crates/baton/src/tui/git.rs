//! The current git branch of a session's repo, read from `HEAD` directly
//! (no `git` process), so it can be polled cheaply.

use std::fs;
use std::path::{Path, PathBuf};

/// Longest branch name shown.
const MAX_BRANCH: usize = 64;

/// The branch checked out in the repository containing `dir`; `@<sha>` for a
/// detached `HEAD`. `None` outside a repository or when `HEAD` is unreadable.
pub fn branch(dir: &Path) -> Option<String> {
    let head = fs::read_to_string(git_dir(dir)?.join("HEAD")).ok()?;
    parse_head(&head)
}

/// The git directory of the repository containing `dir`: a `.git` directory,
/// or the target of a `.git` file (`gitdir: <path>`, as in worktrees and
/// submodules).
fn git_dir(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find_map(|d| {
        let dot = d.join(".git");
        if dot.is_dir() {
            return Some(dot);
        }
        let file = fs::read_to_string(&dot).ok()?;
        let target = file.strip_prefix("gitdir:")?.trim();
        Some(d.join(target)) // `join` keeps an absolute target as is
    })
}

/// The branch named by the contents of a `HEAD` file.
pub fn parse_head(head: &str) -> Option<String> {
    let head = head.trim();
    let name = match head.strip_prefix("ref:") {
        Some(r) => {
            let r = r.trim();
            r.strip_prefix("refs/heads/").unwrap_or(r).to_owned()
        }
        None if head.len() >= 7 && head.bytes().all(|b| b.is_ascii_hexdigit()) => {
            format!("@{}", &head[..7])
        }
        None => return None,
    };
    // `HEAD` is outside our control: keep it printable and short.
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_BRANCH)
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_contents() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n").as_deref(),
            Some("main")
        );
        assert_eq!(
            parse_head("ref: refs/heads/feat/quota").as_deref(),
            Some("feat/quota")
        );
        assert_eq!(
            parse_head("da78766c0ffee0000000000000000000000000ab\n").as_deref(),
            Some("@da78766")
        );
        assert_eq!(
            parse_head("ref: refs/heads/a\u{1b}[2Jb").as_deref(),
            Some("a[2Jb")
        );
        for bad in ["", "ref: ", "ref: refs/heads/", "garbage", "abc"] {
            assert_eq!(parse_head(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn finds_the_branch_from_a_subdirectory_and_through_a_gitdir_file() {
        let t = tempfile::tempdir().expect("tempdir");
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).expect("mkdir");
        fs::create_dir_all(repo.join("src/deep")).expect("mkdir");
        fs::write(repo.join(".git/HEAD"), "ref: refs/heads/dev\n").expect("write");
        assert_eq!(branch(&repo).as_deref(), Some("dev"));
        assert_eq!(branch(&repo.join("src/deep")).as_deref(), Some("dev"));

        let wt = t.path().join("wt");
        let wt_git = repo.join(".git/worktrees/wt");
        fs::create_dir_all(&wt).expect("mkdir");
        fs::create_dir_all(&wt_git).expect("mkdir");
        fs::write(wt_git.join("HEAD"), "ref: refs/heads/hotfix\n").expect("write");
        fs::write(wt.join(".git"), format!("gitdir: {}\n", wt_git.display())).expect("write");
        assert_eq!(branch(&wt).as_deref(), Some("hotfix"));
    }

    #[test]
    fn outside_a_repository_there_is_no_branch() {
        let t = tempfile::tempdir().expect("tempdir");
        // Assumes the temp dir is not inside a git checkout.
        assert_eq!(branch(t.path()), None);
    }
}
