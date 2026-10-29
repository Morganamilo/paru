use std::ffi::{OsStr, OsString};
use std::fs::{create_dir_all, write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Result, bail};
use aur_fetch::Fetch;
use tr::tr;

use crate::exec;

// These mirror the refs and objects aur-fetch uses to track what has been reviewed.
const SEEN: &str = "AUR_SEEN";
const UPSTREAM: &str = "HEAD@{u}";
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// A clone to review, optionally limited to a subdirectory of it.
///
/// Whole clones are reviewed through aur-fetch. Subdirectory targets run the same git
/// commands restricted to the subdirectory, so changes elsewhere in the clone are not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewTarget<'a> {
    pub name: &'a str,
    pub subdir: Option<&'a Path>,
}

impl<'a> ReviewTarget<'a> {
    pub fn whole(name: &'a str) -> Self {
        ReviewTarget { name, subdir: None }
    }

    pub fn scoped(name: &'a str, subdir: &'a Path) -> Self {
        let subdir = subdir.strip_prefix("/").unwrap_or(subdir);
        let subdir = (!subdir.as_os_str().is_empty()).then_some(subdir);
        ReviewTarget { name, subdir }
    }

    /// The directory to show the user: the clone itself or the subdirectory within it.
    pub fn dir(&self, fetch: &Fetch) -> PathBuf {
        let clone = fetch.clone_dir.join(self.name);

        match self.subdir {
            Some(subdir) => clone.join(subdir),
            None => clone,
        }
    }

    fn is_unseen(&self, fetch: &Fetch) -> Result<bool> {
        let Some(subdir) = self.subdir else {
            return Ok(!fetch.unseen(&[self.name])?.is_empty());
        };

        if !has_seen(fetch, self.name)? {
            return Ok(true);
        }

        has_subdir_changes(fetch, self.name, subdir)
    }

    fn has_diff(&self, fetch: &Fetch) -> Result<bool> {
        let Some(subdir) = self.subdir else {
            return Ok(!fetch.has_diff(&[self.name])?.is_empty());
        };

        if !has_seen(fetch, self.name)? {
            return Ok(false);
        }

        has_subdir_changes(fetch, self.name, subdir)
    }

    fn diff(&self, fetch: &Fetch, color: bool) -> Result<String> {
        let Some(subdir) = self.subdir else {
            let diff = fetch.diff(&[self.name], color)?;
            return Ok(diff.into_iter().next().unwrap_or_default());
        };

        subdir_diff(fetch, self.name, subdir, color)
    }

    fn save_diff(&self, fetch: &Fetch) -> Result<()> {
        let Some(subdir) = self.subdir else {
            fetch.save_diffs(&[self.name])?;
            return Ok(());
        };

        create_dir_all(&fetch.diff_dir)?;
        let diff = subdir_diff(fetch, self.name, subdir, false)?;
        write(diff_path(fetch, self.name), diff)?;
        Ok(())
    }

    fn add_to_view(&self, fetch: &Fetch, view: &Path, has_diff: bool) -> Result<()> {
        if self.subdir.is_none() {
            let names = [self.name];
            let diffs = if has_diff { &names[..] } else { &[] };
            fetch.make_view(view, &names, diffs)?;
            return Ok(());
        }

        let diff = diff_path(fetch, self.name);
        if has_diff && diff.is_file() {
            symlink(diff, view.join(format!("{}.diff", self.name)))?;
        }

        let dir = self.dir(fetch);
        if dir.is_dir() {
            symlink(&dir, view.join(self.name))?;
        }

        for (file, suffix) in [("PKGBUILD", "PKGBUILD"), (".SRCINFO", "SRCINFO")] {
            let src = dir.join(file);
            if src.is_file() {
                symlink(src, view.join(format!("{}.{}", self.name, suffix)))?;
            }
        }

        Ok(())
    }
}

/// Filters targets, keeping ones that have not yet been seen.
pub fn unseen<'a>(fetch: &Fetch, targets: &[ReviewTarget<'a>]) -> Result<Vec<ReviewTarget<'a>>> {
    let mut ret = Vec::new();

    for target in targets {
        if target.is_unseen(fetch)? {
            ret.push(*target);
        }
    }

    Ok(ret)
}

/// Filters targets, keeping ones that have been seen before and have changes since.
pub fn has_diff<'a>(fetch: &Fetch, targets: &[ReviewTarget<'a>]) -> Result<Vec<ReviewTarget<'a>>> {
    let mut ret = Vec::new();

    for target in targets {
        if target.has_diff(fetch)? {
            ret.push(*target);
        }
    }

    Ok(ret)
}

pub fn diff(fetch: &Fetch, targets: &[ReviewTarget], color: bool) -> Result<Vec<String>> {
    targets
        .iter()
        .map(|target| target.diff(fetch, color))
        .collect()
}

pub fn save_diffs(fetch: &Fetch, targets: &[ReviewTarget]) -> Result<()> {
    for target in targets {
        target.save_diff(fetch)?;
    }

    Ok(())
}

/// Makes a view dir containing symlinks to each target's files and saved diffs.
pub fn make_view(
    fetch: &Fetch,
    view: &Path,
    targets: &[ReviewTarget],
    diffs: &[ReviewTarget],
) -> Result<()> {
    for target in targets {
        target.add_to_view(fetch, view, diffs.contains(target))?;
    }

    Ok(())
}

fn diff_path(fetch: &Fetch, name: &str) -> PathBuf {
    fetch.diff_dir.join(format!("{}.diff", name))
}

fn git_command<S: AsRef<OsStr>>(fetch: &Fetch, name: &str, args: &[S]) -> Command {
    let mut cmd = Command::new(&fetch.git);
    cmd.current_dir(fetch.clone_dir.join(name))
        .args(&fetch.git_flags)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

fn has_seen(fetch: &Fetch, name: &str) -> Result<bool> {
    let mut cmd = git_command(fetch, name, &["rev-parse", "--quiet", "--verify", SEEN]);
    cmd.stdout(Stdio::null());

    match exec::command_status(&mut cmd)?.code() {
        0 => Ok(true),
        1 => Ok(false),
        code => bail!(tr!("{} exited with code {}", exec::command_err(&cmd), code)),
    }
}

/// Whether upstream changed anything in the subdir since it was last seen.
fn has_subdir_changes(fetch: &Fetch, name: &str, subdir: &Path) -> Result<bool> {
    let range = OsString::from(format!("{}...{}", SEEN, UPSTREAM));
    let mut args = vec!["diff".into(), "--quiet".into(), range, "--".into()];
    args.extend(pathspecs(subdir));
    let mut cmd = git_command(fetch, name, &args);

    match exec::command_status(&mut cmd)?.code() {
        0 => Ok(false),
        1 => Ok(true),
        code => bail!(tr!("{} exited with code {}", exec::command_err(&cmd), code)),
    }
}

/// The log and diff of upstream changes to the subdir, in the same format as aur-fetch.
fn subdir_diff(fetch: &Fetch, name: &str, subdir: &Path, color: bool) -> Result<String> {
    let color = OsString::from(if color {
        "--color=always"
    } else {
        "--color=never"
    });

    let log_range = OsString::from(format!("..{}", UPSTREAM));
    let mut log_args = vec!["log".into(), log_range, color.clone(), "--".into()];
    log_args.push(subdir.into());
    let log = exec::command_output(&mut git_command(fetch, name, &log_args))?;

    let base = if has_seen(fetch, name)? {
        format!("{}...{}", SEEN, UPSTREAM)
    } else {
        format!("{}..{}", EMPTY_TREE, UPSTREAM)
    };
    let mut diff_args = vec![
        "diff".into(),
        "--stat".into(),
        "--patch".into(),
        color,
        base.into(),
        "--".into(),
    ];
    diff_args.extend(pathspecs(subdir));
    let diff = exec::command_output(&mut git_command(fetch, name, &diff_args))?;

    let mut ret = String::from_utf8_lossy(&log.stdout).into_owned();
    ret.push_str(&String::from_utf8_lossy(&diff.stdout));
    ret.push('\n');
    Ok(ret)
}

/// Pathspecs matching the subdir, minus any .SRCINFO files in it.
fn pathspecs(subdir: &Path) -> [OsString; 2] {
    let mut exclude = OsString::from(":(exclude,glob)");
    exclude.push(subdir);
    exclude.push("/**/.SRCINFO");
    [subdir.into(), exclude]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::read_link;

    const REPO: &str = "repo";

    struct Fixture {
        _dir: tempfile::TempDir,
        upstream: PathBuf,
        fetch: Fetch,
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }

    fn commit_file(upstream: &Path, file: &str, contents: &str, message: &str) {
        let path = upstream.join(file);
        create_dir_all(path.parent().unwrap()).unwrap();
        write(path, contents).unwrap();
        git(upstream, &["add", "."]);
        git(upstream, &["commit", "-q", "-m", message]);
    }

    /// An upstream with packages in `a/` and `b/`, cloned into the fetch cache.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let upstream = dir.path().join("upstream");
        create_dir_all(&upstream).unwrap();
        git(&upstream, &["init", "-q", "-b", "master"]);
        commit_file(&upstream, "a/PKGBUILD", "pkgname=a\n", "add a");
        commit_file(&upstream, "b/PKGBUILD", "pkgname=b\n", "add b");

        let fetch = Fetch::with_cache_dir(dir.path().join("cache"));
        create_dir_all(&fetch.clone_dir).unwrap();
        let upstream_url = upstream.to_str().unwrap();
        git(&fetch.clone_dir, &["clone", "-q", upstream_url, REPO]);

        Fixture {
            _dir: dir,
            upstream,
            fetch,
        }
    }

    impl Fixture {
        fn fetch_upstream(&self) {
            git(&self.fetch.clone_dir.join(REPO), &["fetch", "-q"]);
        }
    }

    #[test]
    fn scoped_strips_leading_slash() {
        let target = ReviewTarget::scoped(REPO, Path::new("/pkgbuilds/foo"));

        assert_eq!(target.subdir, Some(Path::new("pkgbuilds/foo")));
    }

    #[test]
    fn scoped_to_root_reviews_whole_clone() {
        assert_eq!(
            ReviewTarget::scoped(REPO, Path::new("/")),
            ReviewTarget::whole(REPO)
        );
    }

    #[test]
    fn changes_outside_subdir_are_not_reviewed() {
        let fixture = fixture();
        fixture.fetch.mark_seen(&[REPO]).unwrap();
        commit_file(
            &fixture.upstream,
            "b/PKGBUILD",
            "pkgname=b\npkgver=2\n",
            "bump b",
        );
        fixture.fetch_upstream();

        let scoped = [ReviewTarget::scoped(REPO, Path::new("a"))];
        let whole = [ReviewTarget::whole(REPO)];

        assert!(unseen(&fixture.fetch, &scoped).unwrap().is_empty());
        assert!(has_diff(&fixture.fetch, &scoped).unwrap().is_empty());
        assert_eq!(has_diff(&fixture.fetch, &whole).unwrap(), whole);
    }

    #[test]
    fn diff_only_shows_subdir_changes() {
        let fixture = fixture();
        fixture.fetch.mark_seen(&[REPO]).unwrap();
        commit_file(
            &fixture.upstream,
            "a/PKGBUILD",
            "pkgname=a\npkgver=2\n",
            "bump a",
        );
        commit_file(
            &fixture.upstream,
            "b/PKGBUILD",
            "pkgname=b\npkgver=2\n",
            "bump b",
        );
        fixture.fetch_upstream();

        let targets = [ReviewTarget::scoped(REPO, Path::new("a"))];
        let changed = has_diff(&fixture.fetch, &targets).unwrap();
        let diffs = diff(&fixture.fetch, &changed, false).unwrap();

        assert_eq!(changed, targets);
        assert!(diffs[0].contains("bump a"));
        assert!(diffs[0].contains("a/PKGBUILD"));
        assert!(!diffs[0].contains("bump b"));
        assert!(!diffs[0].contains("b/PKGBUILD"));
    }

    #[test]
    fn diff_ignores_srcinfo_in_subdir() {
        let fixture = fixture();
        fixture.fetch.mark_seen(&[REPO]).unwrap();
        commit_file(
            &fixture.upstream,
            "a/.SRCINFO",
            "pkgbase = a\n",
            "add srcinfo",
        );
        commit_file(
            &fixture.upstream,
            "a/PKGBUILD",
            "pkgname=a\npkgver=2\n",
            "bump a",
        );
        fixture.fetch_upstream();

        let targets = [ReviewTarget::scoped(REPO, Path::new("a"))];
        let diffs = diff(&fixture.fetch, &targets, false).unwrap();

        assert!(diffs[0].contains("a/PKGBUILD"));
        assert!(!diffs[0].contains("pkgbase = a"));
    }

    #[test]
    fn unseen_subdir_is_shown_in_full() {
        let fixture = fixture();
        let targets = [ReviewTarget::scoped(REPO, Path::new("a"))];

        assert_eq!(unseen(&fixture.fetch, &targets).unwrap(), targets);
        assert!(has_diff(&fixture.fetch, &targets).unwrap().is_empty());
        assert_eq!(
            targets[0].dir(&fixture.fetch),
            fixture.fetch.clone_dir.join(REPO).join("a")
        );
    }

    #[test]
    fn view_links_only_the_subdir() {
        let fixture = fixture();
        fixture.fetch.mark_seen(&[REPO]).unwrap();
        commit_file(
            &fixture.upstream,
            "a/PKGBUILD",
            "pkgname=a\npkgver=2\n",
            "bump a",
        );
        fixture.fetch_upstream();

        let targets = [ReviewTarget::scoped(REPO, Path::new("a"))];
        let view = tempfile::tempdir().unwrap();
        save_diffs(&fixture.fetch, &targets).unwrap();
        make_view(&fixture.fetch, view.path(), &targets, &targets).unwrap();

        let subdir = fixture.fetch.clone_dir.join(REPO).join("a");
        let saved_diff = std::fs::read_to_string(view.path().join("repo.diff")).unwrap();
        assert_eq!(read_link(view.path().join(REPO)).unwrap(), subdir);
        assert_eq!(
            read_link(view.path().join("repo.PKGBUILD")).unwrap(),
            subdir.join("PKGBUILD")
        );
        assert!(saved_diff.contains("bump a"));
    }
}
