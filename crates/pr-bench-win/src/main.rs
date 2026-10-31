//! `pr-bench-win`: the `pr-bench` command line with the real Windows host (`WindowsEngineHost`).
//!
//! Same behaviour as `pr-bench` (all of it lives in [`pr_bench::cli`]); the only difference is the host, so a profile run
//! decodes with WIC instead of failing with "not wired". Running a profile takes real measurements: only run it attended
//! (W10-B2), never in CI.

use pr_bench::cli::{self, EngineBackend, SystemToolEnvironment};
use pr_bench_win::WindowsEngineHost;
use pr_core::io::physical::PhysicalFileSystem;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    let mut backend = EngineBackend::new(WindowsEngineHost::new());
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
