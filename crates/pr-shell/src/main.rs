//! Skeleton entry point. The real window lands after the Phase 0 UI spike (docs/adr/R0001).

fn main() {
    let paths = pr_core::app_paths::AppPaths::from_environment();
    println!("PhotoReviewRs skeleton; config: {}", paths.config_file.display());
}
