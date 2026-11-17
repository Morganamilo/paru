#![cfg(feature = "mock")]

pub mod common;

const AURACLE_LINK: &str = "\x1b]8;;https://test.com/pkgbase/auracle-git\x1b\\\
                            auracle-git-r316.dfbd170-1\x1b]8;;\x1b\\";
const POLYBAR_LINK: &str =
    "\x1b]8;;https://test.com/pkgbase/polybar\x1b\\polybar-3.5.6-1\x1b]8;;\x1b\\";

#[test]
fn failed_aur_builds_link_their_pkgbases() {
    let (_tmp, ret, stderr) = common::run_normal_capturing(&[
        "-Sdd",
        "--color",
        "always",
        "--makepkg",
        "/usr/bin/false",
        "auracle-git",
        "polybar",
    ])
    .unwrap();
    assert_eq!(ret, 1, "{stderr}");
    assert!(
        stderr.lines().any(|l| l
            == format!("\x1b[31merror:\x1b[0m failed to download sources for '{AURACLE_LINK}': ")),
        "{stderr}"
    );
    assert_eq!(
        stderr.lines().last(),
        Some(format!("error: packages failed to build: {AURACLE_LINK}  {POLYBAR_LINK}").as_str()),
        "{stderr}"
    );
}

#[test]
fn failed_aur_package_build_links_its_pkgbase() {
    use std::os::unix::fs::PermissionsExt;

    let scripts = tempfile::TempDir::new().unwrap();
    let makepkg = scripts.path().join("makepkg");
    std::fs::write(
        &makepkg,
        "#!/bin/sh\ncase \" $* \" in *\" -feA \"*) exit 1 ;; esac\nexec makepkg \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&makepkg, std::fs::Permissions::from_mode(0o755)).unwrap();

    let (_tmp, ret, stderr) = common::run_normal_capturing(&[
        "-Sdd",
        "--color",
        "always",
        "--makepkg",
        makepkg.to_str().unwrap(),
        "auracle-git",
    ])
    .unwrap();
    assert_eq!(ret, 1, "{stderr}");
    assert!(
        stderr
            .lines()
            .any(|l| l == format!("\x1b[31merror:\x1b[0m failed to build '{AURACLE_LINK}': ")),
        "{stderr}"
    );
}

#[test]
fn unsatisfied_aur_build_links_its_pkgbase() {
    let (_tmp, ret, stderr) = common::run_normal_capturing(&[
        "-S",
        "--color",
        "always",
        "--pacman",
        "/usr/bin/true",
        "auracle-git",
    ])
    .unwrap();
    assert_eq!(ret, 1, "{stderr}");
    assert!(
        stderr.lines().any(|l| l
            == format!(
                "\x1b[31merror:\x1b[0m can't build {AURACLE_LINK}, deps not satisfied: \
                 meson  git  perl  gtest  gmock"
            )),
        "{stderr}"
    );
}

mod normal {
    use crate::common::run_normal as run;
    include!("common/tests.rs");
}

mod combined {
    use crate::common::run_combined as run;
    include!("common/tests.rs");
}

#[cfg(feature = "mock_chroot")]
mod chroot {
    use crate::common::run_chroot as run;
    include!("common/tests.rs");
}

mod repo {
    use crate::common::run_repo as run;
    include!("common/tests.rs");
}

#[cfg(feature = "mock_chroot")]
mod repo_chroot {
    use crate::common::run_repo_chroot as run;
    include!("common/tests.rs");
}
