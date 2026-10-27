//! `armls`, the language server for Apple AArch64 assembly. It speaks LSP
//! over stdin and stdout; editors start it. See docs/design.md#language-server.

#[cfg(test)]
mod fuzz;
mod ide;
#[cfg(test)]
mod perf;
mod server;
mod workspace;

use std::process::ExitCode;

const HELP: &str = "\
armls: language server for AArch64 assembly on macOS

Editors start armls and talk to it over stdin and stdout. See the editors/
directory in the arm-tools repository for setup.

Usage:
  armls             Run the server on stdin and stdout
  armls --version   Print the version
  armls --help      Print this help
";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("--version" | "-V") => {
            println!(
                "armls {} (instruction tables from LLVM {})",
                env!("CARGO_PKG_VERSION"),
                arm_isa::llvm_version()
            );
            return ExitCode::SUCCESS;
        }
        Some("--help" | "-h") => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprint!("armls: unknown argument '{other}'\n\n{HELP}");
            return ExitCode::from(2);
        }
    }
    let (connection, io_threads) = lsp_server::Connection::stdio();
    let result = server::run(&connection);
    drop(connection);
    let joined = io_threads.join();
    match (result, joined) {
        // The protocol says exit with 0 after shutdown, 1 without it.
        (Ok(true), Ok(())) => ExitCode::SUCCESS,
        (Ok(false), Ok(())) => ExitCode::from(1),
        (Err(error), _) => {
            eprintln!("armls: {error}");
            ExitCode::from(1)
        }
        (_, Err(error)) => {
            eprintln!("armls: {error}");
            ExitCode::from(1)
        }
    }
}
