#![cfg(feature = "mock")]

mod common;

use alpm::PackageReason;
use common::*;

#[tokio::test]
async fn pkgbuild() {
    std::env::set_current_dir("testdata/clone/pkg").unwrap();
    let (tmp, ret) = run(&["-Ui"]).await.unwrap();
    assert_eq!(ret, 0);
    let alpm = alpm(&tmp).unwrap();

    let db = alpm.localdb();

    let pacaur = db.pkg("pacaur").unwrap();
    let auracle = db.pkg("auracle-git").unwrap();
    let pkg = db.pkg("pkg").unwrap();

    assert_eq!(pkg.reason(), PackageReason::Explicit);
    assert_eq!(auracle.reason(), PackageReason::Depend);
    assert_eq!(pacaur.reason(), PackageReason::Depend);
}
