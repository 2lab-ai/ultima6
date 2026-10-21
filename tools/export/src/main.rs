//! `u6-export`: the command line of the exporter (see the library doc).

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let a = match u6_export::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "u6-export: {e}\n{}", u6_export::USAGE);
            return ExitCode::from(2);
        }
    };
    match u6_export::run(&a) {
        Ok(line) => {
            let _ = writeln!(std::io::stdout(), "{line}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "u6-export: {e}");
            ExitCode::FAILURE
        }
    }
}
