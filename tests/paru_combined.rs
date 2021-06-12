#![cfg(feature = "mock")]

mod common;

use alpm::PackageReason;
use common::*;

#[tokio::test]
async fn pacaur() {
    let (tmp, ret) = run_combined(&["-S", "pacaur"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();

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

    assert_eq!(pacaur.reason(), PackageReason::Explicit);
    assert_eq!(auracle.reason(), PackageReason::Depend);
}

#[tokio::test]
async fn pacaur_as_deps() {
    let (tmp, ret) = run_combined(&["-S", "pacaur", "--asdeps"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();

    assert_eq!(pacaur.reason(), PackageReason::Depend);
    assert_eq!(auracle.reason(), PackageReason::Depend);
}

#[tokio::test]
async fn pacaur_as_exp() {
    let (tmp, ret) = run_combined(&["-S", "pacaur", "--asexplicit"])
        .await
        .unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();

    assert_eq!(pacaur.reason(), PackageReason::Explicit);
    assert_eq!(auracle.reason(), PackageReason::Explicit);
}

#[tokio::test]
async fn pacaur_no_deps() {
    let (tmp, ret) = run_combined(&["-Sdd", "pacaur"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    db.pkg("pacaur").unwrap();
    db.pkg("auracle-git").unwrap_err();
}

#[tokio::test]
async fn pacaur_assume() {
    let (tmp, ret) = run_combined(&["-S", "--assume-installed=auracle-git", "pacaur"])
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
    let (tmp, ret) = run_combined(&["-Sua"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    let polybar = db.pkg("polybar").unwrap();
    assert_eq!(polybar.version().as_str(), "3.5.6-1");
}

#[tokio::test]
async fn update_ignore() {
    let (tmp, ret) = run_combined(&["-Sua", "--ignore=polybar"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    let polybar = db.pkg("polybar").unwrap();
    assert_eq!(polybar.version().as_str(), "1.0.0-1");
}

#[tokio::test]
async fn update_repo() {
    let (tmp, ret) = run_combined(&["-Su", "--repo"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();
    let db = alpm.localdb();
    let polybar = db.pkg("polybar").unwrap();
    assert_eq!(polybar.version().as_str(), "1.0.0-1");
}

#[tokio::test]
async fn no_exist() {
    let (_, ret) = run_combined(&["-S", "aaaaaaaaaa"]).await.unwrap();
    assert_eq!(ret, 1);
}

#[tokio::test]
async fn no_exist_r() {
    let (_, ret) = run_combined(&["-S", "--repo", "pacaur"]).await.unwrap();
    assert_eq!(ret, 1);
}

#[tokio::test]
async fn no_exist_a() {
    let (_, ret) = run_combined(&["-Sa", "pacman"]).await.unwrap();
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
