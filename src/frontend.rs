use crate::diagnostic::{Diagnostic, Span};

// Keep frontend recursion independent of the calling thread's stack size.
// Parser limits bound the syntax and trees passed to later recursive phases.
pub(crate) fn run<T: Send>(work: impl FnOnce() -> T + Send) -> Result<T, Diagnostic> {
    return std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("crust-frontend".to_owned())
            .stack_size(16 * 1024 * 1024)
            .spawn_scoped(scope, work)
            .map_err(|error| {
                Diagnostic::new(
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        column: 1,
                    },
                    format!("could not start compiler worker: {error}"),
                )
            })?;
        return Ok(worker
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)));
    });
}
