//! Port of `tests/PhotoReview.Imaging.Tests/PreloadPrefetchTests.cs` (C# baseline d92d6aae, 86 lines): the source-bytes
//! prefetch runs only for images without a disk-cached preview, and never for RAW sources.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.

mod common;

use std::sync::{Arc, Mutex};

use common::preload_fakes::*;
use pr_imaging::preload_order::PreloadWindow;

const IMAGE_COUNT: usize = 20;

#[test]
fn prefetch_skips_images_with_disk_cached_preview() {
    // C#: PreloadPrefetchTests.Prefetch_SkipsImagesWithDiskCachedPreview
    let fs = fake_fs();
    let entries = entries(&fs, IMAGE_COUNT, "r7-prefetch");
    let target = FakeTarget::new(&fs);
    for (i, entry) in entries.iter().enumerate() {
        if i % 2 == 0 {
            target.set_disk_cached(entry.path());
        }
    }
    let prefetched: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&prefetched);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(4, PreloadWindow::DEFAULT)
            .full_folder_threshold(1)
            .prefetch(move |path, _cancel| {
                lock(&sink).push(path.to_owned());
                Ok(())
            }),
    );

    assert!(finished(&scheduler.preload_around(0)));

    assert_eq!(
        IMAGE_COUNT - 1,
        target.started().len(),
        "every image is still preloaded"
    );
    let mut expected: Vec<String> = (1..IMAGE_COUNT)
        .filter(|i| i % 2 != 0)
        .map(|i| entries[i].path().to_owned())
        .collect();
    expected.sort_by_key(|p| p.to_ascii_uppercase());
    let mut actual = lock(&prefetched).clone();
    actual.sort_by_key(|p| p.to_ascii_uppercase());
    assert_eq!(expected, actual);
}

#[test]
fn prefetch_skips_raw_sources() {
    // C#: PreloadPrefetchTests.Prefetch_SkipsRawSources
    let fs = fake_fs();
    let entries = entries_with_extension(&fs, IMAGE_COUNT, "r7-raw", "cr3");
    let target = FakeTarget::new(&fs);
    let prefetched: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&prefetched);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(4, PreloadWindow::DEFAULT)
            .full_folder_threshold(1)
            .prefetch(move |path, _cancel| {
                lock(&sink).push(path.to_owned());
                Ok(())
            }),
    );

    assert!(finished(&scheduler.preload_around(0)));

    assert!(lock(&prefetched).is_empty());
    assert_eq!(IMAGE_COUNT - 1, target.started().len());
}
