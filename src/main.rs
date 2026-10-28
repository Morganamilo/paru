use paru::run;
use std::process::exit;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    // SAFETY: no thread exists yet; the runtime below must stay after this call.
    let future = unsafe { run(&args) };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed building the Tokio runtime");
    let ret = runtime.block_on(future);
    exit(ret);
}
