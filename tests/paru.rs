#![cfg(feature = "mock")]

pub mod common;

#[test]
fn search_aur_url_names_the_package_of_a_split_base() {
    let (_tmp, ret, stdout) =
        common::run_normal_capturing_stdout(&["-Sss", "split-base-docs"]).unwrap();
    assert_eq!(ret, 0, "{stdout}");
    assert!(
        stdout
            .lines()
            .any(|l| l == "    AUR URL : https://test.com/packages/split-base-docs"),
        "{stdout}"
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
