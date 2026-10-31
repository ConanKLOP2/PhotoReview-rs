//! `pr-bench` command line. Port of `tools/PhotoReview.Benchmark.Cli/Program.cs` (C# baseline d92d6aae).
//!
//! All behaviour lives in [`pr_bench::cli`] so integration tests can drive it with fakes; this file only wires the
//! real streams, file system and backend. The process exit code is the return value of [`pr_bench::cli::run`].

use pr_bench::cli::{self, EngineBackend, SystemToolEnvironment, UnwiredEngineHost};
use pr_core::io::physical::PhysicalFileSystem;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    // The engine backend is wired (real catalog, executor, workload runner, report writers). The platform host (WIC decoder,
    // payload codec, real Recycle Bin) cannot be named from this crate (pr-platform-win -> pr-app -> pr-bench), so profile runs
    // fail with the "not wired" message here; the `pr-bench-win` binary (crate `pr-bench-win`) supplies the real Windows `EngineHost`.
    let mut backend = EngineBackend::new(UnwiredEngineHost);
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
