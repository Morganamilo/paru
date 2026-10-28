use paru::run;
use std::process::exit;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    // The process is still single-threaded when configuration changes its environment.
    let future = unsafe { run(&args) };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let ret = runtime.block_on(future);
    exit(ret);
}
