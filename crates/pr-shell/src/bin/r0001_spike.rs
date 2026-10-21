//! Phase 0 UI spike entry point (docs/adr/R0001).
//!
//! ```text
//! r0001-spike <folder> [--measure N] [--interval-ms MS] [--size WxH] [--full]
//! r0001-spike --generate <dir> <count> [WxH]     (synthetic photo-like JPEGs for the measurement)
//! ```
//! Keys: Left / Right switch, wheel zoom, drag pan, `F` fit, `1` 100%, Esc quit.

#[cfg(windows)]
fn main() {
    use std::path::PathBuf;
    use std::time::Instant;

    let process_start = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--generate") {
        let (Some(dir), Some(count)) = (args.get(1), args.get(2).and_then(|c| c.parse::<usize>().ok())) else {
            eprintln!("usage: r0001-spike --generate <dir> <count> [WxH]");
            std::process::exit(2);
        };
        let (w, h) = args.get(3).and_then(|s| parse_size(s)).unwrap_or((6000, 4000));
        if let Err(e) = generate(&PathBuf::from(dir), count, w, h) {
            eprintln!("generate failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    let mut folder: Option<PathBuf> = None;
    let mut measure = None;
    let mut interval_ms = 200;
    let mut client = (1600, 900);
    let mut full_only = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--measure" => measure = it.next().and_then(|v| v.parse::<usize>().ok()),
            "--interval-ms" => interval_ms = it.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(interval_ms),
            "--full" => full_only = true,
            "--size" => {
                client = it
                    .next()
                    .and_then(|v| parse_size(v))
                    .map_or(client, |(w, h)| (w as i32, h as i32))
            }
            other => folder = Some(PathBuf::from(other)),
        }
    }
    let Some(folder) = folder else {
        eprintln!("usage: r0001-spike <folder> [--measure N] [--interval-ms MS] [--size WxH] [--full]");
        std::process::exit(2);
    };
    let opts = pr_shell::spike::viewer::Options {
        folder,
        measure,
        interval_ms,
        client,
        full_only,
    };
    if let Err(e) = pr_shell::spike::viewer::run(&opts, process_start) {
        eprintln!("spike failed: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// Writes up to 4 distinct JPEGs through WIC and copies them to reach `count` files (the copies are real files, read
/// from disk like any others).
#[cfg(windows)]
fn generate(dir: &std::path::Path, count: usize, w: u32, h: u32) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let unique = count.min(4);
    let mut bases = Vec::new();
    for i in 0..unique {
        let path = dir.join(format!("img_{i:04}.jpg"));
        pr_platform_win::spike_ui::write_test_jpeg(&path, w, h, i as u32 + 1)?;
        bases.push(path);
    }
    for i in unique..count {
        std::fs::copy(&bases[i % unique], dir.join(format!("img_{i:04}.jpg"))).map_err(|e| e.to_string())?;
    }
    println!("generated {count} files ({w}x{h}) in {}", dir.display());
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("the R0001 spike is Windows-only");
}
