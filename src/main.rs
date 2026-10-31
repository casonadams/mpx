/// Process entry point: tracing, argument parsing and dispatch.
fn main() {
    std::process::exit(mpx::cli::main_entry(std::env::args().collect()));
}
