use lsp_server::Connection;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && (arguments[0] == "--help" || arguments[0] == "-h") {
        println!("Usage: crust-lsp [--stdio]");
        return ExitCode::SUCCESS;
    }
    if !(arguments.is_empty() || arguments.len() == 1 && arguments[0] == "--stdio") {
        eprintln!("Usage: crust-lsp [--stdio]");
        return ExitCode::FAILURE;
    }
    let (connection, threads) = Connection::stdio();
    let result = crust::lsp::serve(&connection);
    drop(connection);
    match result {
        Ok(()) => {
            if let Err(error) = threads.join() {
                eprintln!("crust-lsp: {error}");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("crust-lsp: {error}");
            return ExitCode::FAILURE;
        }
    }
}
