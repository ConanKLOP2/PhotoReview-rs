//! `pr-bench` command line. Port of `tools/PhotoReview.Benchmark.Cli/Program.cs` (C# baseline d92d6aae).
//!
//! All behaviour lives in [`pr_bench::cli`] so integration tests can drive it with fakes; this file only wires the
//! real streams, file system and backend. The process exit code is the return value of [`pr_bench::cli::run`].

use pr_bench::cli::{self, SystemToolEnvironment, UnwiredBackend};
use pr_core::io::physical::PhysicalFileSystem;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    // W7-03d wiring: replace `UnwiredBackend` by the engine-backed backend once W7-01..W7-03c are merged.
    let mut backend = UnwiredBackend;
    let code = cli::run(
        &args,
        &mut out,
        &mut err,
        &mut backend,
        &SystemToolEnvironment,
        &PhysicalFileSystem,
    );
    std::process::exit(code);
}
