//! The spike viewer: folder of JPEGs, arrow-key switching with 1-ahead / 1-behind preload, Fit + source-pixel zoom
//! (wheel) + drag pan, Direct2D bitmap draw, repaint only when invalidated, and an automated measurement mode.
//!
//! Decoding uses the production `WicDirectDecoder` (`pr-platform-win`) and `PixelBuffer` (`pr-imaging`). All `unsafe`
//! is in `pr_platform_win::spike_ui`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView};
use pr_imaging::decoder_traits::ImageDecoder;
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::PixelLayout;
use pr_platform_win::spike_ui::{self, Canvas, Ctx, Input, Key, SpikeApp, WakeHandle, WindowConfig};
use pr_platform_win::wic::decoder::WicDirectDecoder;

use super::stats::summary;
use super::view::View;

/// How the spike was started.
#[derive(Debug, Clone)]
pub struct Options {
    pub folder: PathBuf,
    /// `Some(n)`: switch through `n` images automatically, print the numbers and quit.
    pub measure: Option<usize>,
    /// Pause between two automatic switches (the preload runs in it).
    pub interval_ms: u32,
    /// Client size in physical pixels.
    pub client: (i32, i32),
    /// Decode every image at full resolution (no viewport-sized preview tier).
    pub full_only: bool,
}

enum Outcome {
    Ready(Arc<dyn DecodedImageView>),
    Failed,
    Skipped,
}

struct Done {
    index: usize,
    decode_ms: f64,
    outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Interactive,
    Settle,
    Switching,
    Zooming,
    Idle,
}

#[derive(Default)]
struct Samples {
    switch_ms: Vec<f64>,
    switch_excl_present_ms: Vec<f64>,
    present_ms: Vec<f64>,
    switch_hit_ms: Vec<f64>,
    switch_miss_ms: Vec<f64>,
    preload_decode_ms: Vec<f64>,
    miss_decode_ms: Vec<f64>,
    upload_ms: Vec<f64>,
    upgrade_ms: Vec<f64>,
    zoom_ms: Vec<f64>,
    go_ms: Vec<f64>,
    warm_ms: Vec<f64>,
    hits: usize,
    misses: usize,
    late_uploads: usize,
    errors: usize,
}

struct Viewer {
    files: Vec<PathBuf>,
    index: usize,
    decoder: Arc<WicDirectDecoder>,
    cache: HashMap<usize, Arc<dyn DecodedImageView>>,
    inflight: HashSet<usize>,
    failed: HashSet<usize>,
    jobs: Sender<usize>,
    results: Receiver<Done>,
    wanted: Arc<AtomicUsize>,
    view: View,
    start: Instant,
    first_frame: Option<Duration>,
    drawn_image: bool,
    phase: Phase,
    measure_total: usize,
    done_steps: usize,
    interval_ms: u32,
    samples: Samples,
    idle_start: Option<(u64, Instant)>,
    last_switch_ms: f64,
    client: (i32, i32),
    /// Decode box of the preview tier (unbounded = full only).
    decode_box: DecodeBox,
    /// Full-resolution image of the current index, decoded on the first zoom.
    full: Option<(usize, Arc<dyn DecodedImageView>)>,
}

const FULL_KEY: u64 = 1 << 40;

fn memory() -> Arc<dyn MemoryInfoSource> {
    Arc::new(|| MemoryInfo {
        total_available: 64 * 1024 * 1024 * 1024,
        load: 0,
    })
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// JPEG files of `folder`, sorted by file name ignoring case.
fn scan(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("jpg") || x.eq_ignore_ascii_case("jpeg"))
        })
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    Ok(files)
}

/// Cyclic distance between two indices of a folder of `len` files.
fn cyclic_distance(a: usize, b: usize, len: usize) -> usize {
    let d = a.abs_diff(b);
    d.min(len - d)
}

fn decode(decoder: &WicDirectDecoder, path: &Path, decode_box: DecodeBox) -> Result<Arc<dyn DecodedImageView>, String> {
    decoder
        .decode(&DecodeRequest::with_box(path, decode_box))
        .map_err(|e| e.describe())
}

impl Viewer {
    fn new(opts: &Options, files: Vec<PathBuf>, wake: WakeHandle, start: Instant) -> Self {
        let decoder = Arc::new(WicDirectDecoder::new(PixelBufferImageCodec::shared(), None, memory()));
        let (jobs, job_rx) = channel::<usize>();
        let (result_tx, results) = channel::<Done>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let wanted = Arc::new(AtomicUsize::new(0));
        let len = files.len();
        for _ in 0..2 {
            let (job_rx, result_tx, decoder, wanted, files) = (
                Arc::clone(&job_rx),
                result_tx.clone(),
                Arc::clone(&decoder),
                Arc::clone(&wanted),
                files.clone(),
            );
            let decode_box = if opts.full_only {
                DecodeBox::UNBOUNDED
            } else {
                DecodeBox::new(opts.client.0, opts.client.1)
            };
            thread::spawn(move || {
                loop {
                    let next = job_rx.lock().unwrap_or_else(PoisonError::into_inner).recv();
                    let Ok(index) = next else { return };
                    let t0 = Instant::now();
                    // A superseded request (the user moved on) is dropped before it reads the file.
                    let outcome = if cyclic_distance(index, wanted.load(Ordering::Relaxed), len) > 1 {
                        Outcome::Skipped
                    } else {
                        match decode(&decoder, &files[index], decode_box) {
                            Ok(img) => Outcome::Ready(img),
                            Err(_) => Outcome::Failed,
                        }
                    };
                    if result_tx
                        .send(Done {
                            index,
                            decode_ms: ms(t0),
                            outcome,
                        })
                        .is_err()
                    {
                        return;
                    }
                    wake.wake();
                }
            });
        }
        Self {
            files,
            index: 0,
            decoder,
            cache: HashMap::new(),
            inflight: HashSet::new(),
            failed: HashSet::new(),
            jobs,
            results,
            wanted,
            view: View::default(),
            start,
            first_frame: None,
            drawn_image: false,
            phase: if opts.measure.is_some() {
                Phase::Settle
            } else {
                Phase::Interactive
            },
            measure_total: opts.measure.unwrap_or(0),
            done_steps: 0,
            interval_ms: opts.interval_ms,
            samples: Samples::default(),
            idle_start: None,
            last_switch_ms: 0.0,
            client: opts.client,
            decode_box: if opts.full_only {
                DecodeBox::UNBOUNDED
            } else {
                DecodeBox::new(opts.client.0, opts.client.1)
            },
            full: None,
        }
    }

    fn len(&self) -> usize {
        self.files.len()
    }

    fn image_size(&self) -> Option<(f32, f32)> {
        self.cache
            .get(&self.index)
            .map(|i| (i.original_width() as f32, i.original_height() as f32))
    }

    /// Queues 1-ahead / 1-behind decodes and drops everything further away.
    fn schedule_preload(&mut self, ctx: &mut Ctx<'_>) {
        self.wanted.store(self.index, Ordering::Relaxed);
        let len = self.len();
        let window = [(self.index + len - 1) % len, self.index, (self.index + 1) % len];
        self.cache.retain(|k, _| window.contains(k));
        if self.full.as_ref().is_some_and(|(i, _)| *i != self.index) {
            self.full = None;
        }
        let mut keep: Vec<u64> = window.iter().map(|&i| i as u64).collect();
        if self.full.is_some() {
            keep.push(FULL_KEY);
        }
        ctx.prepare(|c| c.retain_bitmaps(&keep));
        for &i in &[window[2], window[0]] {
            if !self.cache.contains_key(&i) && !self.failed.contains(&i) && self.inflight.insert(i) {
                let _ = self.jobs.send(i);
            }
        }
    }

    /// Moves by `delta` (cyclic). Returns whether the decoded image was already cached.
    fn go(&mut self, delta: isize, ctx: &mut Ctx<'_>) -> bool {
        let len = self.len() as isize;
        self.index = ((self.index as isize + delta).rem_euclid(len)) as usize;
        self.view.reset();
        let mut hit = self.cache.contains_key(&self.index);
        if !hit && !self.failed.contains(&self.index) {
            let t0 = Instant::now();
            match decode(&self.decoder, &self.files[self.index], self.decode_box) {
                Ok(img) => {
                    self.samples.miss_decode_ms.push(ms(t0));
                    self.cache.insert(self.index, img);
                }
                Err(_) => {
                    self.failed.insert(self.index);
                }
            }
        }
        if self.failed.contains(&self.index) {
            hit = false;
        }
        self.schedule_preload(ctx);
        hit
    }

    /// Uploads the bitmap of `index` if its image is cached and the bitmap is missing.
    fn upload(&mut self, canvas: &mut Canvas<'_>, key: u64, image: Option<Arc<dyn DecodedImageView>>) -> bool {
        if canvas.has_bitmap(key) {
            return false;
        }
        let Some(image) = image else { return false };
        let Some(pixels) = image.platform_image().as_pixels().cloned() else {
            return false;
        };
        let Ok(bytes) = pixels.bytes() else { return false };
        let t0 = Instant::now();
        let opaque = pixels.layout() == PixelLayout::Bgr32;
        let (w, h, stride) = (pixels.width(), pixels.height(), pixels.stride());
        if canvas
            .upload(key, w as u32, h as u32, stride as u32, bytes, opaque)
            .is_ok()
        {
            self.samples.upload_ms.push(ms(t0));
            return true;
        }
        false
    }

    fn label(&self) -> String {
        let name = self.files[self.index]
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let zoom = self.image_size().map_or(String::new(), |img| {
            let vp = (self.client.0 as f32, self.client.1 as f32);
            let scale = self.view.effective_scale(img, vp);
            format!(
                "{}  {}x{}  {}{:.0}%",
                name,
                img.0,
                img.1,
                if self.view.is_fit() { "Fit " } else { "" },
                scale * 100.0
            )
        });
        let status = if self.failed.contains(&self.index) {
            format!("{name}: decode failed")
        } else {
            zoom
        };
        let sample = if self.phase == Phase::Interactive {
            "   Ảnh: Tiếng Việt có dấu — ăâêôơư đ"
        } else {
            ""
        };
        format!("{}/{}  {}{}", self.index + 1, self.len(), status, sample)
    }

    fn apply_input(&mut self, input: Input, ctx: &mut Ctx<'_>) {
        let vp = ctx.client_size();
        let image = self.image_size();
        match input {
            Input::Key(Key::Escape) => ctx.quit(),
            Input::Key(Key::Left) => self.step(-1, ctx),
            Input::Key(Key::Right) => self.step(1, ctx),
            Input::Key(Key::Fit) => self.view.reset(),
            Input::Key(Key::Actual) => {
                if let Some(img) = image {
                    self.view.actual_size(img);
                    self.ensure_full(ctx);
                }
            }
            Input::Wheel { notches, x, y } => {
                if let Some(img) = image {
                    self.view.zoom_at(notches, (x, y), img, vp);
                    self.ensure_full(ctx);
                }
            }
            Input::Drag { dx, dy } => {
                if let Some(img) = image {
                    self.view.pan((dx, dy), img);
                }
            }
            Input::Resized => {}
        }
    }

    /// Zoomed in: decode the full-resolution image of the current file once (synchronously, like the first zoom of C#).
    fn ensure_full(&mut self, ctx: &mut Ctx<'_>) {
        let needs = !self.view.is_fit()
            && self.full.as_ref().is_none_or(|(i, _)| *i != self.index)
            && self.cache.get(&self.index).is_some_and(|i| i.downscaled());
        if !needs {
            return;
        }
        let t0 = Instant::now();
        if let Ok(img) = decode(&self.decoder, &self.files[self.index], DecodeBox::UNBOUNDED) {
            self.full = Some((self.index, Arc::clone(&img)));
            ctx.prepare(|c| {
                self.upload(c, FULL_KEY, Some(img));
            });
            self.samples.upgrade_ms.push(ms(t0));
        }
    }

    /// Interactive arrow key: switch, repaint when the system is idle, show the last switch time in the title.
    fn step(&mut self, delta: isize, ctx: &mut Ctx<'_>) {
        let t0 = Instant::now();
        self.go(delta, ctx);
        ctx.render_now(|c| self.paint(c));
        self.last_switch_ms = ms(t0);
        let title = format!(
            "PhotoReviewRs spike - {} - switch {:.1} ms",
            self.label(),
            self.last_switch_ms
        );
        ctx.set_title(&title);
    }

    fn print_report(&self) {
        let s = &self.samples;
        let first = self.first_frame.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
        println!(
            "spike.images={} switches={} interval_ms={} client={}x{}",
            self.len(),
            self.measure_total,
            self.interval_ms,
            self.client.0,
            self.client.1
        );
        if let Some(img) = self.image_size() {
            println!("spike.image_size={}x{}", img.0, img.1);
        }
        println!("spike.first_frame_ms={first:.1}");
        println!("spike.switch {}", summary(&s.switch_ms));
        println!("spike.switch_excl_present {}", summary(&s.switch_excl_present_ms));
        println!("spike.present_call {}", summary(&s.present_ms));
        if std::env::var_os("PHOTOREVIEW_RS_SPIKE_RAW").is_some() {
            let raw: Vec<String> = s.switch_ms.iter().map(|v| format!("{v:.1}")).collect();
            println!("spike.switch_raw_ms={}", raw.join(","));
        }
        println!("spike.gpu_warm {}", summary(&s.warm_ms));
        println!("spike.go_part {}", summary(&s.go_ms));
        println!("spike.switch_hit {}", summary(&s.switch_hit_ms));
        println!("spike.switch_miss {}", summary(&s.switch_miss_ms));
        println!(
            "spike.cache hits={} misses={} late_uploads={} errors={}",
            s.hits, s.misses, s.late_uploads, s.errors
        );
        println!("spike.preload_decode {}", summary(&s.preload_decode_ms));
        println!("spike.miss_decode {}", summary(&s.miss_decode_ms));
        println!("spike.full_res_upgrade_on_first_zoom {}", summary(&s.upgrade_ms));
        println!("spike.bitmap_upload {}", summary(&s.upload_ms));
        println!("spike.zoom_pan_frame {}", summary(&s.zoom_ms));
        if let Some((cpu0, wall0)) = self.idle_start {
            let cpu_s = (spike_ui::process_cpu_time_100ns().saturating_sub(cpu0)) as f64 / 1.0e7;
            let wall_s = wall0.elapsed().as_secs_f64();
            println!(
                "spike.idle window_s={wall_s:.2} cpu_s={cpu_s:.4} cpu_percent_of_one_core={:.3}",
                cpu_s / wall_s * 100.0
            );
        }
    }

    fn measure_tick(&mut self, ctx: &mut Ctx<'_>) {
        match self.phase {
            Phase::Settle => {
                self.phase = Phase::Switching;
                ctx.set_timer(self.interval_ms.max(1));
            }
            Phase::Switching => {
                let t0 = Instant::now();
                let hit = self.go(1, ctx);
                self.samples.go_ms.push(ms(t0));
                let late_before = self.samples.upload_ms.len();
                ctx.render_now(|c| self.paint(c));
                let dt = ms(t0);
                if self.failed.contains(&self.index) {
                    self.samples.errors += 1;
                } else {
                    self.samples.switch_ms.push(dt);
                    let present = ctx.last_present_ms();
                    self.samples.present_ms.push(present);
                    self.samples.switch_excl_present_ms.push(dt - present);
                    if hit {
                        self.samples.hits += 1;
                        self.samples.switch_hit_ms.push(dt);
                    } else {
                        self.samples.misses += 1;
                        self.samples.switch_miss_ms.push(dt);
                    }
                    if self.samples.upload_ms.len() > late_before {
                        self.samples.late_uploads += 1;
                    }
                }
                self.done_steps += 1;
                if self.done_steps >= self.measure_total {
                    self.phase = Phase::Zooming;
                    self.done_steps = 0;
                    self.view.reset();
                }
                ctx.set_timer(if self.phase == Phase::Zooming {
                    300
                } else {
                    self.interval_ms.max(1)
                });
            }
            Phase::Zooming => {
                let (vw, vh) = ctx.client_size();
                let k = self.done_steps;
                let input = match k / 10 {
                    0 => Input::Wheel {
                        notches: 1.0,
                        x: vw * 0.4,
                        y: vh * 0.4,
                    },
                    1 => Input::Drag { dx: 40.0, dy: 25.0 },
                    _ => Input::Wheel {
                        notches: -1.0,
                        x: vw * 0.5,
                        y: vh * 0.5,
                    },
                };
                let t0 = Instant::now();
                self.apply_input(input, ctx);
                ctx.render_now(|c| self.paint(c));
                self.samples.zoom_ms.push(ms(t0));
                self.done_steps += 1;
                if self.done_steps >= 30 {
                    self.phase = Phase::Idle;
                    self.idle_start = Some((spike_ui::process_cpu_time_100ns(), Instant::now()));
                    ctx.set_timer(3000);
                } else {
                    ctx.set_timer(20);
                }
            }
            Phase::Idle => {
                self.print_report();
                ctx.quit();
            }
            Phase::Interactive => {}
        }
    }
}

impl SpikeApp for Viewer {
    fn on_create(&mut self, ctx: &mut Ctx<'_>) {
        let t0 = Instant::now();
        match decode(&self.decoder, &self.files[0], self.decode_box) {
            Ok(img) => {
                self.cache.insert(0, img);
                self.samples.miss_decode_ms.push(ms(t0));
            }
            Err(_) => {
                self.failed.insert(0);
            }
        }
        self.schedule_preload(ctx);
        if self.phase == Phase::Settle {
            ctx.set_timer(1500);
        }
    }

    fn on_input(&mut self, input: Input, ctx: &mut Ctx<'_>) {
        if self.phase != Phase::Interactive && !matches!(input, Input::Key(Key::Escape) | Input::Resized) {
            return;
        }
        self.client = {
            let (w, h) = ctx.client_size();
            (w as i32, h as i32)
        };
        if matches!(input, Input::Key(Key::Left | Key::Right)) {
            self.apply_input(input, ctx);
            return;
        }
        self.apply_input(input, ctx);
        ctx.invalidate();
    }

    fn paint(&mut self, canvas: &mut Canvas<'_>) {
        let (vw, vh) = canvas.size();
        self.client = (vw as i32, vh as i32);
        let index = self.index;
        if let Some(preview) = self.cache.get(&index).cloned() {
            // Fit uses the viewport-sized preview; a zoom uses the full-resolution image once it is decoded.
            let full = self
                .full
                .as_ref()
                .filter(|_| !self.view.is_fit())
                .map(|(_, img)| Arc::clone(img));
            let (key, shown) = match full {
                Some(img) => (FULL_KEY, img),
                None => (index as u64, preview),
            };
            self.upload(canvas, key, Some(Arc::clone(&shown)));
            if let Some(img) = self.image_size() {
                let layout = self.view.layout(img, (vw, vh));
                // `layout.src` is in original pixels; the preview bitmap is smaller by this factor.
                let f = shown.pixel_width() as f32 / img.0;
                let src = layout.src.map(|v| v * f);
                canvas.draw_bitmap(key, layout.dest, src, layout.nearest);
                self.drawn_image = true;
            }
        }
        if self.phase == Phase::Interactive {
            let label = self.label();
            canvas.draw_text(&label, 12.0, 8.0, vw - 24.0, 28.0);
        }
    }

    fn on_wake(&mut self, ctx: &mut Ctx<'_>) {
        while let Ok(done) = self.results.try_recv() {
            self.inflight.remove(&done.index);
            match done.outcome {
                Outcome::Ready(img) => {
                    let len = self.len();
                    if cyclic_distance(done.index, self.index, len) <= 1 {
                        self.samples.preload_decode_ms.push(done.decode_ms);
                        self.cache.insert(done.index, img);
                        let index = done.index;
                        // Upload while the UI thread is idle, off the key -> frame path.
                        let warm = std::env::var_os("PHOTOREVIEW_RS_SPIKE_NOWARM").is_none();
                        let cached = self.cache.get(&index).cloned();
                        ctx.prepare(|c| {
                            self.upload(c, index as u64, cached);
                            if warm {
                                let t0 = Instant::now();
                                c.warm(index as u64);
                                self.samples.warm_ms.push(ms(t0));
                            }
                        });
                    }
                }
                Outcome::Failed => {
                    self.failed.insert(done.index);
                }
                Outcome::Skipped => {}
            }
        }
    }

    fn on_timer(&mut self, ctx: &mut Ctx<'_>) {
        ctx.set_timer(0);
        self.measure_tick(ctx);
    }

    fn after_paint(&mut self) {
        if self.first_frame.is_none() && self.drawn_image {
            self.first_frame = Some(self.start.elapsed());
        }
    }
}

/// Opens the window and runs until it closes (interactive) or the measurement ends.
///
/// # Errors
/// A text describing the failure (folder, no JPEG, window creation).
pub fn run(opts: &Options, process_start: Instant) -> Result<(), String> {
    let files = scan(&opts.folder)?;
    if files.is_empty() {
        return Err(format!("{}: no .jpg files", opts.folder.display()));
    }
    let config = WindowConfig {
        title: "PhotoReviewRs spike".to_owned(),
        client_width: opts.client.0,
        client_height: opts.client.1,
        no_activate: opts.measure.is_some() && std::env::var_os("PHOTOREVIEW_RS_SPIKE_ACTIVATE").is_none(),
    };
    let opts = opts.clone();
    spike_ui::run(&config, move |wake| {
        Box::new(Viewer::new(&opts, files, wake, process_start))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyclic_distance_wraps_around_the_folder() {
        assert_eq!(cyclic_distance(0, 9, 10), 1);
        assert_eq!(cyclic_distance(3, 5, 10), 2);
        assert_eq!(cyclic_distance(4, 4, 10), 0);
    }
}
