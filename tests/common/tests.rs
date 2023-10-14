use crate::common::*;
use alpm::PackageReason;

#[tokio::test]
async fn pacaur() {
    let (tmp, ret) = run(&["-S", "pacaur"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();
    assert_in_local_repo(&alpm, "pacaur");
    assert_in_local_repo(&alpm, "auracle-git");

    assert_eq!(pacaur.reason(), PackageReason::Explicit);
    assert_eq!(auracle.reason(), PackageReason::Depend);
}

#[tokio::test]
async fn pacaur_ignore() {
    let (tmp, ret) = run(&["-S", "pacaur", "--ignore=pacaur"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();
    assert_in_local_repo(&alpm, "pacaur");
    assert_in_local_repo(&alpm, "auracle-git");

    assert_eq!(pacaur.reason(), PackageReason::Explicit);
    assert_eq!(auracle.reason(), PackageReason::Depend);
}

#[tokio::test]
async fn pacaur_as_deps() {
    let (tmp, ret) = run(&["-S", "pacaur", "--asdeps"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();
    assert_in_local_repo(&alpm, "pacaur");
    assert_in_local_repo(&alpm, "auracle-git");

    assert_eq!(pacaur.reason(), PackageReason::Depend);
    assert_eq!(auracle.reason(), PackageReason::Depend);
}

#[tokio::test]
async fn pacaur_as_exp() {
    let (tmp, ret) = run(&["-S", "pacaur", "--asexplicit"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();
    assert_in_local_repo(&alpm, "pacaur");
    assert_in_local_repo(&alpm, "auracle-git");

    assert_eq!(pacaur.reason(), PackageReason::Explicit);
    assert_eq!(auracle.reason(), PackageReason::Explicit);
}

#[tokio::test]
async fn pacaur_no_deps() {
    let (tmp, ret) = run(&["-Sdd", "pacaur"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    db.pkg("pacaur").unwrap();
    db.pkg("auracle-git").unwrap_err();
    assert_in_local_repo(&alpm, "pacaur");
}

#[tokio::test]
async fn pacaur_assume() {
    let (tmp, ret) = run(&["-S", "--assume-installed=auracle-git", "pacaur"])
        .await
        .unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    db.pkg("pacaur").unwrap();
    db.pkg("auracle-git").unwrap_err();
}

#[tokio::test]
async fn update() {
    let (tmp, ret) = run(&["-Sua"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    let polybar = db.pkg("polybar").unwrap();
    assert_eq!(polybar.version().as_str(), "3.5.6-1");
}

#[tokio::test]
async fn update_ignore() {
    let (tmp, ret) = run(&["-Sua", "--ignore=polybar"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    let polybar = db.pkg("polybar").unwrap();
    assert_eq!(polybar.version().as_str(), "1.0.0-1");
}

#[tokio::test]
async fn update_repo() {
    let (tmp, ret) = run(&["-Su", "--repo"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    let polybar = db.pkg("polybar").unwrap();
    assert_eq!(polybar.version().as_str(), "1.0.0-1");
}

#[tokio::test]
async fn no_exist() {
    let (_, ret) = run(&["-S", "aaaaaaaaaa"]).await.unwrap();
    assert_eq!(ret, 1);
}

#[tokio::test]
async fn no_exist_r() {
    let (_, ret) = run(&["-S", "--repo", "pacaur"]).await.unwrap();
    assert_eq!(ret, 1);
}

#[tokio::test]
async fn no_exist_a() {
    let (_, ret) = run(&["-Sa", "pacman"]).await.unwrap();
    assert_eq!(ret, 1);
}

#[tokio::test]
async fn repo_ignore() {
    let (tmp, ret) = run(&["-S", "i3-wm", "--ignore=i3-wm"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    db.pkg("i3-wm").unwrap();
}

#[tokio::test]
async fn pkgbuild() {
    let (tmp, ret) = run(&["-Bi", "testdata/clone/pkg"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();
    let pkg = db.pkg("pkg").unwrap();
    assert_in_local_repo(&alpm, "pkg");
    assert_in_local_repo(&alpm, "auracle-git");
    assert_in_local_repo(&alpm, "pacaur");

    assert_eq!(pkg.reason(), PackageReason::Explicit);
    assert_eq!(auracle.reason(), PackageReason::Depend);
    assert_eq!(pacaur.reason(), PackageReason::Depend);
}

#[tokio::test]
async fn pkgbuild_repo() {
    let (tmp, ret) = run(&["-S", "a"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let a = db.pkg("a").unwrap();
    let b = db.pkg("b").unwrap();
    let c = db.pkg("c").unwrap();
    assert_in_local_repo(&alpm, "a");
    assert_in_local_repo(&alpm, "b");
    assert_in_local_repo(&alpm, "c");

    assert_eq!(a.reason(), PackageReason::Explicit);
    assert_eq!(b.reason(), PackageReason::Depend);
    assert_eq!(c.reason(), PackageReason::Depend);
}

#[tokio::test]
async fn devel() {
    let (tmp, ret) = run(&["-Sua", "--devel"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();
    let a = db.pkg("devel").unwrap();
    assert_eq!(a.version().as_str(), "2-1");
}
