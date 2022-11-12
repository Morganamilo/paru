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
