#![cfg(feature = "mock")]

pub mod common;

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
