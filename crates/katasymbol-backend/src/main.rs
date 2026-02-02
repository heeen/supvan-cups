//! CUPS backend: katasymbol
//!
//! Dispatch based on argc:
//!   - 1 arg (just program name): discovery mode - list available printers
//!   - 6-7 args: print job mode
//!
//! CUPS backend protocol:
//!   argv[0] = backend name
//!   argv[1] = job-id
//!   argv[2] = user
//!   argv[3] = title
//!   argv[4] = copies
//!   argv[5] = options
//!   argv[6] = filename (optional, stdin if omitted)

mod discover;
mod print_job;

use std::process;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() == 1 {
        // Discovery mode: list printers
        discover::print_discovery();
    } else if args.len() >= 6 {
        // Print job mode
        if let Err(e) = print_job::run_print_job() {
            eprintln!("ERROR: {e}");
            process::exit(1);
        }
    } else {
        eprintln!(
            "Usage: {} job-id user title copies options [filename]",
            args[0]
        );
        process::exit(1);
    }
}
