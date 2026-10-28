#![cfg(target_os = "linux")]

use std::fs;
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
        fs::create_dir_all(repo.join(".git")).unwrap();
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
             [options]\nBuildDir = {1}/configured\n[fixture]\nUrl = https://paru.invalid/fixture.git\n",
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
    use std::net::TcpListener;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let pacman_conf = pacman_conf(dir);
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    proxy.set_nonblocking(true).unwrap();
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
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match proxy.accept() {
                Ok((stream, _)) => break stream,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "configured proxy received no request"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("{err}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
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
