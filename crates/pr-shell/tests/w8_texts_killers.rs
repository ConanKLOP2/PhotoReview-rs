#![cfg(windows)]

//! New Rust composition regressions: the C# container needs no adapter twin.
//! Fixed inputs exercise forwarding without reading user configuration or clocks.

// This existing fixture exports seams for several registry test binaries.
#[allow(dead_code)]
#[path = "common/composition_fixture.rs"]
mod composition_fixture;

use pr_app::services::prewarm::PrewarmPreviews;
use pr_app::viewmodels::main::FormatContextSource;
use pr_core::time::DateTimeUtc;
use pr_imaging::decode_types::DecodeBox;
use pr_platform_win::{culture::utc_to_local, shell_startup::locale::group_separator};
use pr_shell::composition::adapters::{LazyDecoderFactory, LazyPrewarmPreviews, LocalizedFormat};

struct UnusedClipboard;

impl pr_app::services::contracts::ClipboardService for UnusedClipboard {
    fn try_set_text(&self, _: &str) -> bool {
        panic!("Debug must not access the clipboard");
    }
}

struct UnusedFolderDialog;

impl pr_platform_win::dialogs::folder_dialog::ShellFolderDialog for UnusedFolderDialog {
    fn show(
        &self,
        _: pr_platform_win::win32::structs::Hwnd,
        _: &str,
        _: Option<&str>,
    ) -> Result<Option<String>, pr_platform_win::win32::structs::Hresult> {
        panic!("Debug must not open a folder picker");
    }
}

struct UnusedTaskDialogs;

impl pr_platform_win::dialogs::task_dialog::TaskDialogPort for UnusedTaskDialogs {
    fn show(
        &self,
        _: pr_platform_win::win32::structs::Hwnd,
        _: &str,
        _: &str,
        _: pr_platform_win::dialogs::task_dialog::TaskDialogKind,
    ) -> i32 {
        panic!("Debug must not show a dialog");
    }
}

#[test]
fn shared_adapters_debug_preserve_names_without_calling_the_ports() {
    // New: adapters.rs:43,100,118, replace Debug::fmt with Ok(Default::default()).
    use pr_platform_win::dialogs::{
        dialog_service::{LazySecondaryPort, Win32DialogService},
        folder_dialog::FolderPicker,
    };
    use pr_platform_win::shell_viewing::presentation_sink::{ShellPresentationHub, Win32PresentationSinkFactory};
    use pr_shell::composition::adapters::{SharedClipboard, SharedDialogService, Win32SinkFactoryAdapter};
    use std::rc::Rc;

    let dialogs = SharedDialogService(Rc::new(Win32DialogService::with_task_dialogs(
        Box::new(|| panic!("Debug must not resolve the owner")),
        Box::new(UnusedTaskDialogs),
        FolderPicker::new(Rc::new(UnusedFolderDialog), Box::new(|| 0)),
        Rc::new(LazySecondaryPort::new(Box::new(|| {
            panic!("Debug must not resolve dialogs")
        }))),
    )));
    let clipboard = SharedClipboard(Rc::new(UnusedClipboard));
    let sink = Win32SinkFactoryAdapter(Rc::new(Win32PresentationSinkFactory::new(Rc::new(
        ShellPresentationHub::new(),
    ))));
    assert_eq!(format!("{dialogs:?}"), "SharedDialogService { .. }");
    assert_eq!(format!("{clipboard:?}"), "SharedClipboard { .. }");
    assert_eq!(format!("{sink:?}"), "Win32SinkFactoryAdapter { .. }");
}

#[test]
fn localized_format_new_and_default_forward_the_locale_separator() {
    // New: adapters.rs:273, replace with_context with (); construction reads the Windows locale.
    let expected = group_separator();
    for format in [LocalizedFormat::new(), LocalizedFormat::default()] {
        let mut calls = 0;
        format.with_context(&mut |context| {
            calls += 1;
            assert_eq!(context.group_separator, expected);
        });
        assert_eq!(calls, 1);
    }
}

#[test]
fn localized_format_fixed_separator_is_preserved_exactly() {
    // New: adapters.rs:261, replace with_group_separator with Default::default().
    for separator in ["", "cx-texts-separator"] {
        let format = LocalizedFormat::with_group_separator(separator.to_owned());
        let mut calls = 0;
        format.with_context(&mut |context| {
            calls += 1;
            assert_eq!(context.group_separator, separator);
        });
        assert_eq!(calls, 1);
    }
}

#[test]
fn localized_format_to_local_time_forwards_fixed_instants() {
    // New: adapters.rs:285, replace to_local_time with Default::default().
    // The platform conversion is the oracle for this forwarding adapter; no wall clock is read.
    let format = LocalizedFormat::new();
    for date in ["2024-01-15T12:34:56Z", "2024-07-15T12:34:56Z"] {
        let (utc, _) = DateTimeUtc::parse_json(date).expect("fixed ISO instant");
        let expected = utc_to_local(utc);
        assert_ne!(expected, DateTimeUtc::MIN);
        assert_eq!(format.to_local_time(utc), expected);
    }
}

#[test]
fn lazy_adapters_debug_name_the_wrapper_without_resolving_services() {
    // New: adapters.rs:153,203,247, replace Debug::fmt with Ok(Default::default()).
    let (services, _root, _) = composition_fixture::shared_services();
    let previews = LazyPrewarmPreviews::new(services.clone());
    let decoders = LazyDecoderFactory::new(services);
    let format = LocalizedFormat::with_group_separator("separator".to_owned());
    assert_eq!(format!("{previews:?}"), "LazyPrewarmPreviews { .. }");
    assert_eq!(format!("{decoders:?}"), "LazyDecoderFactory { .. }");
    assert_eq!(format!("{format:?}"), "LocalizedFormat { .. }");
}

#[test]
fn lazy_previews_forward_cache_key_path_version_and_target_box() {
    // New: adapters.rs:170, replace current_cache_key with Ok(Default::default()).
    // The file's actual version is compared to the service's result, never to a clock or assumed mtime.
    let (services, root, _) = composition_fixture::shared_services();
    let path = root.join("forwarding.jpg");
    std::fs::write(&path, b"fixed source bytes").expect("TempRoot is writable");
    let path = path.to_str().expect("UTF-8 TempRoot path");
    let previews = LazyPrewarmPreviews::new(services.clone());
    let service = services.preview_service().expect("fake service graph builds");
    for target in [None, Some(DecodeBox::new(731, 419))] {
        let expected =
            PrewarmPreviews::current_cache_key(service.as_ref(), path, target).expect("the fixture source exists");
        let actual = previews.current_cache_key(path, target).expect("forwarded key");
        assert_eq!(actual, expected);
        assert_eq!(actual.length(), 18);
    }
    let absent = root.join("absent.jpg");
    let absent = absent.to_str().expect("UTF-8 TempRoot path");
    assert_eq!(
        previews.current_cache_key(absent, None),
        PrewarmPreviews::current_cache_key(service.as_ref(), absent, None)
    );
}

#[test]
fn lazy_previews_forward_the_decoder_error_through_the_returned_handle() {
    // New: adapters.rs:185, replace start_viewer_preview with Default::default().
    // The timeout is a hang guard, not an assertion about decode speed.
    use pr_core::model::DecoderBackend;
    use pr_imaging::{
        decoder_traits::{DecodeError, ImageDecoder, ImageDecoderFactory},
        service::PreviewError,
    };
    use std::{sync::Arc, time::Duration};

    struct ErrorFactory;
    impl ImageDecoderFactory for ErrorFactory {
        fn create(&self, _: DecoderBackend) -> Arc<dyn ImageDecoder> {
            Arc::new(composition_fixture::FixtureDecoder)
        }

        fn is_registered(&self, _: DecoderBackend) -> bool {
            true
        }
    }

    let (services, root, _) =
        composition_fixture::shared_services_with(None, Some(Arc::new(|_| Arc::new(ErrorFactory))), None);
    let path = root.join("decoder-error.jpg");
    std::fs::write(&path, b"fixed source bytes").expect("TempRoot is writable");
    let path = path.to_str().expect("UTF-8 TempRoot path");
    let previews = LazyPrewarmPreviews::new(services);
    let key = previews.current_cache_key(path, None).expect("source exists");
    let result = previews
        .start_viewer_preview(path, &key)
        .wait_timeout(Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("preview worker exceeded the hang guard"));
    match result {
        Err(PreviewError::Decode(error)) => assert!(matches!(
            error.as_ref(),
            DecodeError::InvalidData { message } if message == "unused fixture decoder"
        )),
        Err(other) => panic!("expected the injected decoder error, got {other:?}"),
        Ok(_) => panic!("the injected decoder cannot produce an image"),
    }
}
