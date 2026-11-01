//! The `od` binary; see the library for the commands.

fn main() -> std::process::ExitCode {
    od_cli::main_with(std::env::args_os().collect())
}
