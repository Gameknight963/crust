#[path = "../compiler.rs"]
mod compiler;
#[path = "../compiler_cli.rs"]
mod compiler_cli;
#[path = "../invocation.rs"]
mod invocation;

fn main() -> std::process::ExitCode {
    return compiler_cli::main();
}
