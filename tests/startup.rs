#![cfg(target_os = "linux")]

use std::fs;
#[cfg(not(feature = "mock"))]
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn command(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_paru"));
    cmd.current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("PARU_CONF", dir.join("paru.conf"))
        .env("PARU_TEST_CAPTURE", dir.join("capture"))
        .env("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"))
        .env("TOKIO_WORKER_THREADS", "2")
        .env("LC_ALL", "C");
    for key in [
        "AURDEST",
        "PKGEXT",
        "PARU_DEBUG",
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "REQUEST_METHOD",
    ] {
        cmd.env_remove(key);
    }
    cmd
}

fn output(cmd: &mut Command) -> Output {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("paru timed out: {output:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

fn pacman_conf(dir: &Path) -> PathBuf {
    for name in ["root", "db", "gnupg"] {
        fs::create_dir(dir.join(name)).unwrap();
    }
    fs::write(
        dir.join("pacman.conf"),
        format!(
            "[options]\nRootDir = {0}/root\nDBPath = {0}/db\nGPGDir = {0}/gnupg\n\
             LogFile = {0}/pacman.log\nArchitecture = {1}\nSigLevel = Never\nParallelDownloads = 1\n",
            dir.display(),
            std::env::consts::ARCH,
        ),
    )
    .unwrap();
    script(dir, "pacman-conf", "/bin/cat pacman.conf")
}

fn git(dir: &Path, args: &[&str]) -> Output {
    let output = output(
        Command::new("git")
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@localhost",
            ])
            .args(args),
    );
    assert!(output.status.success(), "{output:?}");
    output
}

struct PkgbuildReview {
    tmp: tempfile::TempDir,
    committed_srcinfo: bool,
}

impl PkgbuildReview {
    fn new(options: &str, committed_srcinfo: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let remote = dir.join("remote");
        fs::create_dir(&remote).unwrap();
        git(&remote, &["init", "-b", "master"]);
        let pacman_conf = pacman_conf(dir);
        let pager = script(
            dir,
            "pager",
            "cat > reviewed\nprintf 'review\\n' >> \"$PARU_TEST_CAPTURE\"",
        );
        let makepkg = script(
            dir,
            "makepkg",
            "printf 'makepkg\\n' >> \"$PARU_TEST_CAPTURE\"\n\
             test \"$*\" = --printsrcinfo\n\
             . ./PKGBUILD\n\
             printf 'pkgbase = fixture\\n\\tpkgver = %s\\n\\tpkgrel = 1\\n\\tarch = any\\npkgname = fixture\\n' \"$pkgver\"",
        );
        let forbidden = script(dir, "forbidden", "exit 99");
        fs::write(
            dir.join("paru.conf"),
            format!(
                "[bin]\nPacmanConf = {0}\nPager = {1}\nMakepkg = {2}\n\
                 Pacman = {3}\nSudo = {3}\n[options]\nBuildDir = {4}/build\n\
                 AurUrl = http://127.0.0.1:9\n[fixture]\nUrl = {5}\n{options}",
                pacman_conf.display(),
                pager.display(),
                makepkg.display(),
                forbidden.display(),
                dir.display(),
                url::Url::from_directory_path(&remote).unwrap(),
            ),
        )
        .unwrap();
        Self {
            tmp,
            committed_srcinfo,
        }
    }

    fn clone_dir(&self) -> PathBuf {
        self.tmp.path().join("build/repo/fixture")
    }

    fn push(&self, version: u32) {
        let remote = self.tmp.path().join("remote");
        fs::write(
            remote.join("PKGBUILD"),
            format!(
                "pkgname=fixture\npkgver={version}\npkgrel=1\n\
                 printf 'source-{version}\\n' >> \"$PARU_TEST_CAPTURE\"\n"
            ),
        )
        .unwrap();
        if self.committed_srcinfo {
            fs::write(
                remote.join(".SRCINFO"),
                format!(
                    "pkgbase = fixture\n\tpkgver = {version}\n\tpkgrel = 1\n\tarch = any\npkgname = fixture\n"
                ),
            )
            .unwrap();
        }
        git(&remote, &["add", "."]);
        git(&remote, &["commit", "-m", "fixture"]);
    }

    fn run(&self, args: &[&str], answer: &str) -> Output {
        let dir = self.tmp.path();
        fs::write(dir.join("capture"), "").unwrap();
        fs::write(dir.join("reviewed"), "").unwrap();
        fs::write(dir.join("answer"), answer).unwrap();
        output(
            command(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .stdin(fs::File::open(dir.join("answer")).unwrap())
                .args(["--pkgbuilds", "--color", "never"])
                .args(args),
        )
    }

    fn refresh(&self, answer: &str) -> Output {
        self.run(&["-Sy"], answer)
    }

    fn trace(&self) -> String {
        fs::read_to_string(self.tmp.path().join("capture")).unwrap()
    }

    fn tree(&self, reference: &str) -> Vec<u8> {
        git(
            &self.clone_dir(),
            &["rev-parse", &format!("{reference}^{{tree}}")],
        )
        .stdout
    }
}

#[test]
fn pkgbuild_refresh_reviews_the_upstream_of_an_unaccepted_clone() {
    let repo = PkgbuildReview::new("GenerateSrcinfo\n", false);
    repo.push(1);
    let output = repo.refresh("n\n");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(repo.trace(), "review\n");

    repo.push(2);
    let output = repo.refresh("y\n");
    assert!(output.status.success(), "{output:?}");
    let reviewed = fs::read_to_string(repo.tmp.path().join("reviewed")).unwrap();
    assert!(reviewed.contains("pkgver=2"), "{reviewed}");
    assert!(!reviewed.contains("pkgver=1"), "{reviewed}");
    assert_eq!(repo.trace(), "review\nmakepkg\nsource-2\n");
}

#[test]
fn pkgbuild_refresh_keeps_the_accepted_tree_after_rejection() {
    for answer in ["n\n", ""] {
        let repo = PkgbuildReview::new("GenerateSrcinfo\n", false);
        repo.push(1);
        let output = repo.refresh("y\n");
        assert!(output.status.success(), "{output:?}");
        let accepted = repo.tree("HEAD");

        repo.push(2);
        let output = repo.refresh(answer);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(repo.trace(), "review\n");
        assert_eq!(repo.tree("HEAD"), accepted);
        fs::remove_file(repo.clone_dir().join(".SRCINFO")).unwrap();

        let output = repo.run(&["-Si", "fixture/fixture"], "");
        assert!(output.status.success(), "{output:?}");
        assert_eq!(repo.trace(), "makepkg\nsource-1\n");
    }
}

#[test]
fn pkgbuild_refresh_marks_the_merged_tree_as_accepted() {
    let repo = PkgbuildReview::new("GenerateSrcinfo\n", false);
    for version in [1, 2] {
        repo.push(version);
        let output = repo.refresh("y\n");
        assert!(output.status.success(), "{output:?}");
        assert_eq!(repo.tree("AUR_SEEN"), repo.tree("HEAD"));
    }

    let output = repo.refresh("");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(repo.trace(), "makepkg\nsource-2\n");
    assert_eq!(
        fs::read_to_string(repo.tmp.path().join("reviewed")).unwrap(),
        ""
    );
}

#[test]
fn unaccepted_pkgbuild_repositories_are_not_loaded() {
    for committed_srcinfo in [false, true] {
        for answer in ["n\n", ""] {
            let repo = PkgbuildReview::new("", committed_srcinfo);
            repo.push(1);
            let output = repo.refresh(answer);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert_eq!(repo.trace(), "review\n");

            for args in [vec!["-Slq"], vec!["-Si", "fixture/fixture"]] {
                let output = repo.run(&args, "");
                assert!(output.stdout.is_empty(), "{output:?}");
                assert_eq!(repo.trace(), "", "{args:?}: {output:?}");
            }
            for target in ["unrelated", "fixture/fixture"] {
                let output = repo.run(&["-S", target], "y\n");
                assert!(!output.status.success(), "{output:?}");
                assert_eq!(repo.trace(), "", "{target}: {output:?}");
            }
        }
    }
}

#[test]
fn unavailable_pkgbuild_repositories_warn_once_per_command() {
    for downloaded in [false, true] {
        let repo = PkgbuildReview::new("", false);
        repo.push(1);
        if downloaded {
            let output = repo.refresh("n\n");
            assert_eq!(output.status.code(), Some(1), "{output:?}");
        }

        let output = repo.run(&["-Si", "a", "b", "c", "d", "e"], "");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert_eq!(repo.trace(), "");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stderr
                .lines()
                .filter(|line| line.starts_with(":: "))
                .count(),
            1,
            "{stderr}"
        );
    }
}

#[test]
fn pkgbuild_repository_is_not_loaded_after_interrupted_diff_review() {
    let mut repo = PkgbuildReview::new("", true);
    repo.push(1);
    let output = repo.refresh("y\n");
    assert!(output.status.success(), "{output:?}");

    repo.committed_srcinfo = false;
    fs::remove_file(repo.tmp.path().join("remote/.SRCINFO")).unwrap();
    repo.push(2);
    let wrapper = script(
        repo.tmp.path(),
        "interrupt-git",
        "case \"$*\" in\n\
           'diff --stat --patch --cached '*)\n\
             printf 'interrupt\\n' >> \"$PARU_TEST_CAPTURE\"\n\
             kill -KILL \"$PPID\"\n\
             exit 99;;\n\
         esac\n\
         exec git \"$@\"",
    );
    let config = repo.tmp.path().join("paru.conf");
    let original = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        original.replace("[bin]\n", &format!("[bin]\nGit = {}\n", wrapper.display())),
    )
    .unwrap();

    let output = repo.refresh("");
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(repo.trace(), "interrupt\n");
    fs::write(config, original).unwrap();
    assert_eq!(repo.tree("HEAD"), repo.tree("AUR_SEEN"));
    assert!(repo.clone_dir().join(".git/MERGE_HEAD").exists());
    assert!(!repo.clone_dir().join(".SRCINFO").exists());
    assert!(
        fs::read_to_string(repo.clone_dir().join("PKGBUILD"))
            .unwrap()
            .contains("pkgver=2")
    );

    for args in [vec!["-Slq"], vec!["-Si", "fixture/fixture"]] {
        let output = repo.run(&args, "");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert_eq!(repo.trace(), "", "{args:?}: {output:?}");
    }
    let output = repo.run(&["-S", "fixture/fixture"], "y\n");
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(repo.trace(), "");

    git(&repo.clone_dir(), &["merge", "--abort"]);
    let output = repo.run(&["-Slq"], "");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"fixture\n");
}

#[test]
fn pkgbuild_repository_is_not_loaded_during_rebase() {
    let repo = PkgbuildReview::new("", true);
    repo.push(1);
    let output = repo.refresh("y\n");
    assert!(output.status.success(), "{output:?}");
    git(
        &repo.clone_dir(),
        &["commit", "--allow-empty", "-m", "local"],
    );
    let editor = script(
        repo.tmp.path(),
        "sequence-editor",
        "sed -i '1ibreak' \"$1\"",
    );
    git(
        &repo.clone_dir(),
        &[
            "-c",
            &format!("sequence.editor={}", editor.display()),
            "rebase",
            "-i",
            "HEAD~1",
        ],
    );
    assert_eq!(repo.tree("HEAD"), repo.tree("AUR_SEEN"));
    assert!(repo.clone_dir().join(".git/rebase-merge").is_dir());

    let output = repo.run(&["-Slq"], "");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(repo.trace(), "");

    git(&repo.clone_dir(), &["rebase", "--abort"]);
    let output = repo.run(&["-Slq"], "");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"fixture\n");
}

#[test]
fn pkgbuild_skip_review_explicitly_allows_loading_the_repository() {
    let repo = PkgbuildReview::new("SkipReview\n", false);
    repo.push(1);
    let output = repo.refresh("");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(repo.trace(), "makepkg\nsource-1\n");

    let output = repo.run(&["-Slq"], "");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"fixture\n");
    fs::remove_file(repo.clone_dir().join(".SRCINFO")).unwrap();
    let output = repo.run(&["-Si", "fixture/fixture"], "");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(repo.trace(), "makepkg\nsource-1\n");
}

#[test]
fn pkgbuild_refresh_without_confirmation_marks_the_accepted_tree() {
    let repo = PkgbuildReview::new("", false);
    repo.push(1);
    let output = repo.run(&["-Sy", "--noconfirm"], "");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(repo.trace(), "makepkg\nsource-1\n");
    assert_eq!(repo.tree("AUR_SEEN"), repo.tree("HEAD"));
    let output = repo.run(&["-Slq"], "");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"fixture\n");
}

#[test]
fn pkgbuild_url_cache_without_git_metadata_is_not_loaded() {
    for committed_srcinfo in [false, true] {
        let repo = PkgbuildReview::new("", committed_srcinfo);
        repo.push(1);
        fs::create_dir_all(repo.clone_dir()).unwrap();
        for file in ["PKGBUILD", ".SRCINFO"] {
            let source = repo.tmp.path().join("remote").join(file);
            if source.exists() {
                fs::copy(source, repo.clone_dir().join(file)).unwrap();
            }
        }

        for args in [vec!["-Slq"], vec!["-Si", "fixture/fixture"]] {
            let output = repo.run(&args, "");
            assert!(output.stdout.is_empty(), "{output:?}");
            assert_eq!(repo.trace(), "", "{args:?}: {output:?}");
        }
    }
}

#[test]
fn pkgbuild_path_repository_does_not_require_a_review_marker() {
    let repo = PkgbuildReview::new("", false);
    repo.push(1);
    let remote = repo.tmp.path().join("remote");
    let config = repo.tmp.path().join("paru.conf");
    fs::write(
        &config,
        fs::read_to_string(&config).unwrap().replace(
            &format!("Url = {}", url::Url::from_directory_path(&remote).unwrap()),
            &format!("Path = {}", remote.display()),
        ),
    )
    .unwrap();

    let output = repo.run(&["-Slq"], "");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.ends_with(b"fixture\n"), "{output:?}");
    assert_eq!(repo.trace(), "makepkg\nsource-1\n");
}

#[cfg(not(feature = "mock"))]
fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "server received no request");
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("{err}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
}

#[test]
fn config_environment_reaches_help_before_threads_start() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let pacman = script(
        dir,
        "pacman",
        "printf '%s\\n' \"$PARU_TEST_VALUE\" \"$PARU_TEST_NESTED\" \"${AURDEST-unset}\" > \"$PARU_TEST_CAPTURE\"\n\
         set -- /proc/\"$PPID\"/task/*\n\
         test -d \"$1\"\n\
         printf '%s\\n' \"$#\" >> \"$PARU_TEST_CAPTURE\"",
    );
    fs::write(
        dir.join("inner.conf"),
        "PARU_TEST_VALUE = inner\nPARU_TEST_NESTED = nested\nAURDEST =\n",
    )
    .unwrap();
    fs::write(
        dir.join("outer.conf"),
        format!(
            "PARU_TEST_VALUE = outer\nInclude = {}/inner.conf\n",
            dir.display()
        ),
    )
    .unwrap();
    fs::write(
        dir.join("paru.conf"),
        format!(
            "[bin]\nPacman = {}\n[env]\nPARU_TEST_VALUE = first\n\
             Include = {}/outer.conf\nPARU_TEST_VALUE = last\n",
            pacman.display(),
            dir.display(),
        ),
    )
    .unwrap();

    let output = output(
        command(dir)
            .env("PARU_TEST_VALUE", "inherited")
            .args(["-S", "--help"]),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(dir.join("capture")).unwrap(),
        "last\nnested\n\n1\n"
    );
}

#[test]
fn chroot_clears_pkgext_before_reading_pacman_configuration() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let pacman_conf = script(
        dir,
        "pacman-conf",
        "printf '%s\\n' \"${PKGEXT-unset}\" > \"$PARU_TEST_CAPTURE\"\nexit 78",
    );
    fs::write(
        dir.join("paru.conf"),
        format!(
            "[bin]\nPacmanConf = {}\n[env]\nPKGEXT = .pkg.tar.configured\n",
            pacman_conf.display(),
        ),
    )
    .unwrap();

    for chroot in [false, true] {
        let mut cmd = command(dir);
        cmd.env("PKGEXT", ".pkg.tar.inherited").arg("-Q");
        if chroot {
            cmd.arg("--chroot");
        }
        let output = output(&mut cmd);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            fs::read_to_string(dir.join("capture")).unwrap(),
            if chroot {
                "unset\n"
            } else {
                ".pkg.tar.configured\n"
            },
        );
    }
}

#[test]
fn configured_aurdest_and_cli_select_the_cached_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let pacman_conf = pacman_conf(dir);
    for name in ["configured", "environment", "cli"] {
        let repo = dir.join(name).join("repo/fixture");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "master"]);
        fs::write(repo.join("PKGBUILD"), "").unwrap();
        fs::write(
            repo.join(".SRCINFO"),
            format!("pkgbase = {name}\n\tpkgver = 1\n\tpkgrel = 1\npkgname = {name}\n"),
        )
        .unwrap();
    }
    fs::write(
        dir.join("paru.conf"),
        format!(
            "[bin]\nPacmanConf = {0}\n[env]\nAURDEST = {1}/environment\n\
             [options]\nBuildDir = {1}/configured\n\
             [fixture]\nUrl = https://paru.invalid/fixture.git\nSkipReview\n",
            pacman_conf.display(),
            dir.display(),
        ),
    )
    .unwrap();

    for (cli, expected) in [(false, "environment\n"), (true, "cli\n")] {
        let mut cmd = command(dir);
        cmd.args(["-Slq", "--pkgbuilds"]);
        if cli {
            cmd.arg("--builddir").arg(dir.join("cli"));
        }
        let output = output(&mut cmd);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[cfg(not(feature = "mock"))]
#[test]
fn configured_proxy_routes_the_aur_request() {
    use std::io::{BufRead, BufReader, Read, Write};

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let pacman_conf = pacman_conf(dir);
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let aur_url = format!("http://{}", origin.local_addr().unwrap());
    fs::write(
        dir.join("paru.conf"),
        format!(
            "[bin]\nPacmanConf = {}\n[options]\nAurUrl = {}\n[env]\nhttp_proxy = http://{}\n",
            pacman_conf.display(),
            aur_url,
            proxy.local_addr().unwrap(),
        ),
    )
    .unwrap();
    let server = thread::spawn(move || {
        let mut stream = accept(&proxy);
        let mut reader = BufReader::new(&stream);
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        let mut length = 0;
        loop {
            let mut header = String::new();
            assert_ne!(reader.read_line(&mut header).unwrap(), 0);
            if header == "\r\n" {
                break;
            }
            if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let response = serde_json::json!({
            "version": 5, "type": "search", "resultcount": 1,
            "results": [raur::Package {
                name: "startup-proxy-fixture".into(),
                version: "1-1".into(),
                ..Default::default()
            }],
        })
        .to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        (request, String::from_utf8(body).unwrap())
    });

    let output = output(command(dir).args(["-Ssq", "--aur", "startup-proxy-fixture"]));
    let (request, body) = server.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(request, format!("POST {aur_url}/rpc HTTP/1.1\r\n"));
    let params: Vec<_> = url::form_urlencoded::parse(body.as_bytes()).collect();
    assert!(
        params
            .iter()
            .any(|(key, value)| key == "type" && value == "search")
    );
    assert!(
        params
            .iter()
            .any(|(key, value)| key == "arg" && value == "startup-proxy-fixture")
    );
    assert_eq!(output.stdout, b"startup-proxy-fixture\n");
}

fn pkgbuild_copy_repo(dir: &Path, base: &str) {
    let pacman_conf = pacman_conf(dir);
    let forbidden = script(
        dir,
        "forbidden",
        "printf called > \"$PARU_TEST_CAPTURE\"\nexit 99",
    );
    let repo = dir.join("repo");
    for (folder, pkgbase, pkgname) in [("recipe", base, "target"), ("other", "aaa-good", "good")] {
        let path = repo.join(folder);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("PKGBUILD"), "exit 99\n").unwrap();
        fs::write(
            path.join(".SRCINFO"),
            format!("pkgbase = {pkgbase}\n\tpkgver = 1\n\tpkgrel = 1\n\tarch = any\npkgname = {pkgname}\n"),
        )
        .unwrap();
    }
    fs::write(
        dir.join("paru.conf"),
        format!(
            "[bin]\nPacmanConf = {}\nMakepkg = {}\nSudo = {}\n[fixture]\nPath = {}\n",
            pacman_conf.display(),
            forbidden.display(),
            forbidden.display(),
            repo.display(),
        ),
    )
    .unwrap();
}

#[test]
fn pkgbuild_copy_rejects_unsafe_bases_before_removing_anything() {
    for base in [
        "../outside",
        "/absolute",
        "link/outside",
        "outside/",
        "./outside",
        ".",
        "..",
        ".git",
        ".ssh",
        "-outside",
        "out side",
        "out:side",
        "out\\side",
        "café",
        "out\0side",
        "out\x1b]2;name-marker\x07side",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("work");
        fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(tmp.path(), dir.join("link")).unwrap();
        let base = if base == "/absolute" {
            tmp.path().join("outside").to_str().unwrap().to_owned()
        } else {
            base.to_owned()
        };
        pkgbuild_copy_repo(&dir, &base);
        let hidden = matches!(base.as_str(), ".git" | ".ssh");
        let victim = (!base.contains('\0') && !hidden).then(|| dir.join(&base));
        if let Some(victim) = &victim {
            fs::create_dir_all(victim).unwrap();
            fs::write(victim.join("PKGBUILD"), "original package\n").unwrap();
            fs::write(victim.join("marker"), "keep\n").unwrap();
        }
        fs::create_dir(dir.join("aaa-good")).unwrap();
        fs::write(dir.join("aaa-good/PKGBUILD"), "original good package\n").unwrap();

        let output = output(command(&dir).args([
            "-G",
            "--color",
            "never",
            "--pkgbuilds",
            "fixture/good",
            "fixture/target",
        ]));
        assert_eq!(output.status.code(), Some(1), "{base}: {output:?}");
        if let Some(victim) = &victim {
            assert_eq!(
                fs::read(victim.join("marker")).unwrap(),
                b"keep\n",
                "{base}"
            );
        }
        if hidden {
            assert!(!dir.join(&base).exists(), "{base}");
        }
        assert!(
            !output.stderr.iter().any(|byte| [0, 7, 27].contains(byte)),
            "{base:?}: {output:?}"
        );
        assert_eq!(
            fs::read(dir.join("aaa-good/PKGBUILD")).unwrap(),
            b"original good package\n",
            "{base}"
        );
        assert!(!dir.join("capture").exists(), "{base}");
    }
}

#[test]
fn pkgbuild_copy_rejects_overlapping_directories_before_removing_anything() {
    for (cwd, base) in [
        ("repo", "recipe"),
        (".", "repo"),
        ("repo/recipe", "nested"),
        (".", "alias"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        pkgbuild_copy_repo(dir, base);
        let source = dir.join("repo/recipe");
        fs::write(source.join("marker"), "keep source\n").unwrap();
        fs::write(dir.join("repo/PKGBUILD"), "exit 99\n").unwrap();
        fs::write(
            dir.join("repo/.SRCINFO"),
            "pkgbase = root\n\tpkgver = 1\n\tpkgrel = 1\n\tarch = any\npkgname = root\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(&source, dir.join("alias")).unwrap();
        script(dir, "pacman-conf", "/bin/cat \"$PARU_TEST_PACMAN_CONF\"");
        let cwd = dir.join(cwd);
        fs::create_dir(cwd.join("aaa-good")).unwrap();
        fs::write(cwd.join("aaa-good/PKGBUILD"), "original good package\n").unwrap();
        fs::write(
            cwd.join("aaa-good/.SRCINFO"),
            "pkgbase = aaa-good\n\tpkgver = 1\n\tpkgrel = 1\n\tarch = any\npkgname = old-good\n",
        )
        .unwrap();

        let output = output(
            command(dir)
                .current_dir(&cwd)
                .env("PARU_TEST_PACMAN_CONF", dir.join("pacman.conf"))
                .args(["-G", "--pkgbuilds", "fixture/good", "fixture/target"]),
        );
        assert_eq!(output.status.code(), Some(1), "{cwd:?}: {output:?}");
        assert_eq!(fs::read(source.join("marker")).unwrap(), b"keep source\n");
        assert_eq!(fs::read(source.join("PKGBUILD")).unwrap(), b"exit 99\n");
        assert_eq!(
            fs::read(cwd.join("aaa-good/PKGBUILD")).unwrap(),
            b"original good package\n"
        );
        assert!(!dir.join("capture").exists());
        assert!(!source.join("nested").exists());
    }
}

#[test]
fn pkgbuild_copy_keeps_other_selected_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    pkgbuild_copy_repo(dir, "downloaded");
    let repo = dir.join("repo");
    let source = repo.join("aaa-good");
    fs::rename(repo.join("recipe"), &source).unwrap();
    fs::write(source.join("marker"), "keep source\n").unwrap();
    fs::write(source.join("PKGBUILD"), "target recipe\n").unwrap();
    script(dir, "pacman-conf", "/bin/cat \"$PARU_TEST_PACMAN_CONF\"");

    let output = output(
        command(dir)
            .current_dir(&repo)
            .env("PARU_TEST_PACMAN_CONF", dir.join("pacman.conf"))
            .args(["-G", "--pkgbuilds", "fixture/good", "fixture/target"]),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(fs::read(source.join("marker")).unwrap(), b"keep source\n");
    assert_eq!(
        fs::read(source.join("PKGBUILD")).unwrap(),
        b"target recipe\n"
    );
    assert!(!repo.join("downloaded").exists());
    assert!(!dir.join("capture").exists());
}

#[test]
fn pkgbuild_copy_keeps_independent_recipe_directory_names() {
    for base in ["downloaded", "@Mixed_Name+1.0-2"] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        pkgbuild_copy_repo(dir, base);
        let destination = dir.join(base);
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("PKGBUILD"), "original\n").unwrap();
        fs::write(destination.join("marker"), "old\n").unwrap();

        let output = output(command(dir).args(["-G", "--pkgbuilds", "fixture/target"]));
        assert!(output.status.success(), "{base}: {output:?}");
        assert_eq!(
            fs::read(destination.join("PKGBUILD")).unwrap(),
            b"exit 99\n"
        );
        assert!(!destination.join("marker").exists());
        assert!(!dir.join("capture").exists());
    }
}

#[cfg(not(feature = "mock"))]
#[test]
fn package_descriptions_and_urls_escape_terminal_controls() {
    use std::io::{BufRead, BufReader, Read, Write};

    for (name, version, url, description, rendered_name, rendered_version, rendered_description) in [
        (
            "terminal-fixture",
            "1-1",
            "https://example.org/naïve",
            "naïve 日本語 café",
            "terminal-fixture",
            "1-1",
            "naïve 日本語 café",
        ),
        (
            "terminal-fixture\x1b]2;name-marker\x07\x7f\u{009b}",
            "1\x1b]2;version-marker\x07-1",
            "https://example.org/\x1b\\\x1b]2;url-marker\x07",
            "safe\x1b]2;desc-marker\x07tail\x7f\u{009b}",
            r"terminal-fixture\u{1b}]2;name-marker\u{7}\u{7f}\u{9b}",
            r"1\u{1b}]2;version-marker\u{7}-1",
            r"safe\u{1b}]2;desc-marker\u{7}tail\u{7f}\u{9b}",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let pacman_conf = pacman_conf(dir);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        fs::write(
            dir.join("paru.conf"),
            format!(
                "[bin]\nPacmanConf = {}\n[options]\nAurUrl = http://{}\n",
                pacman_conf.display(),
                listener.local_addr().unwrap()
            ),
        )
        .unwrap();
        let server = thread::spawn(move || {
            for _ in 0..7 {
                let mut stream = accept(&listener);
                let mut reader = BufReader::new(&stream);
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                assert_eq!(request, "POST /rpc HTTP/1.1\r\n");
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    assert_ne!(reader.read_line(&mut header).unwrap(), 0);
                    if header == "\r\n" {
                        break;
                    }
                    if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:")
                    {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let info = url::form_urlencoded::parse(&body)
                    .any(|(key, value)| key == "type" && value == "info");
                let response = serde_json::json!({
                    "version": 5,
                    "type": if info { "multiinfo" } else { "search" },
                    "resultcount": 1,
                    "results": [raur::Package {
                        name: name.into(),
                        package_base: "terminal-fixture".into(),
                        version: version.into(),
                        description: Some(description.into()),
                        url: Some(url.into()),
                        ..Default::default()
                    }],
                })
                .to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
        });
        for mode in ["-Ss", "-Sss", "-Si"] {
            for color in ["always", "never"] {
                let output = output(command(dir).args([mode, "--aur", "--color", color, name]));
                assert!(output.status.success(), "{mode} {color}: {output:?}");
                let text = String::from_utf8(output.stdout).unwrap();
                assert!(
                    text.contains(rendered_description),
                    "{mode} {color}: {text:?}"
                );
                assert!(text.contains(rendered_name), "{mode} {color}: {text:?}");
                assert!(text.contains(rendered_version), "{mode} {color}: {text:?}");
                assert!(!text.contains("\x1b]2;url-marker"), "{text:?}");
                assert!(!text.contains("\x1b]2;desc-marker"), "{text:?}");
                assert!(!text.contains(['\x07', '\x7f', '\u{009b}']), "{text:?}");
                if color == "always" {
                    assert!(text.contains("\x1b[1m"), "{text:?}");
                    if mode != "-Si" && !url.contains('\x1b') {
                        assert!(text.contains(&format!("\x1b]8;;{url}\x1b\\")), "{text:?}");
                    }
                } else {
                    assert!(!text.contains('\x1b'), "{text:?}");
                }
                if url.contains('\x1b') {
                    assert!(!text.contains("\x1b]8;;https://example.org/"), "{text:?}");
                }
            }
        }
        let output = output(command(dir).args(["-Ssq", "--aur", "--color", "always", name]));
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, format!("{rendered_name}\n").as_bytes());
        server.join().unwrap();
    }
}

#[test]
fn pkgbuild_search_and_info_escape_terminal_metadata() {
    let repo = PkgbuildReview::new("", true);
    repo.push(1);
    let name = "terminal-fixture\x1b]2;name-marker\x07";
    let version = "1\x1b]2;version-marker\x07";
    let repository = "fixture\x07repo-marker";
    let arch = "x86_64\x1b\x1b\x1b\x1b\x1b\x07";
    let rendered_name = r"terminal-fixture\u{1b}]2;name-marker\u{7}";
    let rendered_version = r"1\u{1b}]2;version-marker\u{7}-1";
    let rendered_repository = r"fixture\u{7}repo-marker";
    let rendered_arch = r"x86_64\u{1b}\u{1b}\u{1b}\u{1b}\u{1b}\u{7}";
    let remote = repo.tmp.path().join("remote");
    fs::write(
        remote.join(".SRCINFO"),
        format!(
            "pkgbase = fixture\n\tpkgver = {version}\n\tpkgrel = 1\n\tarch = any\n\tarch = {arch}\n\
             pkgname = {name}\n\tpkgdesc = naïve 日本語 café\n\
             \turl = https://example.org/\n\tdepends_{arch} = dep\x1b]2;dep-marker\x07\n"
        ),
    )
    .unwrap();
    git(&remote, &["add", ".SRCINFO"]);
    git(&remote, &["commit", "-m", "metadata fixture"]);
    let config = repo.tmp.path().join("paru.conf");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .unwrap()
            .replace("[fixture]", &format!("[{repository}]")),
    )
    .unwrap();
    let output = repo.refresh("y\n");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(repo.trace(), "review\n");

    for mode in ["-Ss", "-Sss", "-Ssq", "-Si"] {
        for color in ["always", "never"] {
            let target = if mode == "-Si" {
                name
            } else {
                "terminal-fixture"
            };
            let output = repo.run(&[mode, "--color", color, target], "");
            assert!(output.stderr.is_empty(), "{mode} {color}: {output:?}");
            assert_eq!(repo.trace(), "");
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains(rendered_name), "{mode} {color}: {text:?}");
            assert!(!text.contains(['\x07', '\x7f', '\u{009b}']), "{text:?}");
            for marker in ["name", "version", "repo", "dep"] {
                assert!(
                    !text.contains(&format!("\x1b]2;{marker}-marker")),
                    "{text:?}"
                );
            }
            if mode == "-Ssq" {
                assert_eq!(text, format!("{rendered_name}\n"));
            } else {
                assert!(text.contains(rendered_version), "{mode} {color}: {text:?}");
                assert!(
                    text.contains(rendered_repository),
                    "{mode} {color}: {text:?}"
                );
            }
            if color == "never" {
                assert!(!text.contains('\x1b'), "{text:?}");
            } else if mode != "-Ssq" {
                assert!(text.contains("\x1b["), "{text:?}");
            }
            if mode == "-Si" {
                assert!(
                    text.contains(&format!("Depends On {rendered_arch}")),
                    "{text:?}"
                );
                assert!(text.contains(r"dep\u{1b}]2;dep-marker\u{7}"), "{text:?}");
                if color == "never" {
                    let widths = text
                        .lines()
                        .filter_map(|line| {
                            line.split_once(": ")
                                .map(|(label, _)| label.chars().count())
                        })
                        .collect::<Vec<_>>();
                    assert!(widths.iter().all(|width| *width == widths[0]), "{text:?}");
                }
            }
        }
    }
}
