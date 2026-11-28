/// Process entry point: tracing, argument parsing and dispatch.
fn main() {
    mpx::cli::panic_log::install_panic_log(mpx::store::state_dir());
    std::process::exit(mpx::cli::main_entry(std::env::args().collect()));
}
