//! Port of the three settings-level tests of C# `JournalDurabilityTests` (baseline 7eaee2a2):
//! `OldConfigWithoutJournalDurability_LoadsAsFast`, `JournalDurability_ParsesLeniently` and
//! `Settings_RoundTrip_PreservesPowerLossSafe_AndProviderReadsLive`.

use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::{Clock, FileSystem};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{OperationJournal, OperationJournalOptions};
use pr_core::model::enums::JournalDurability;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::store::SettingsStore;
use pr_test_support::{FakeClock, FakeFileSystem, RecordingLog};

struct Rig {
    fs: Arc<FakeFileSystem>,
    clock: Arc<FakeClock>,
    paths: AppPaths,
}

impl Rig {
    fn new() -> Self {
        let clock = Arc::new(FakeClock::new());
        Self {
            fs: Arc::new(FakeFileSystem::with_clock(Arc::clone(&clock))),
            clock,
            paths: AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false),
        }
    }

    fn store(&self) -> Arc<SettingsStore> {
        Arc::new(SettingsStore::new(
            self.paths.clone(),
            Arc::clone(&self.fs) as Arc<dyn FileSystem>,
            Arc::new(RecordingLog::new()),
            Arc::clone(&self.clock) as Arc<dyn Clock>,
            Arc::new(EnglishSettingsTexts),
        ))
    }

    fn put(&self, text: &str) {
        self.fs.add_file(&self.paths.config_file, text.as_bytes());
    }
}

// C#: JournalDurabilityTests.OldConfigWithoutJournalDurability_LoadsAsFast
#[test]
fn old_config_without_journal_durability_loads_as_fast() {
    let r = Rig::new();
    let store = r.store();
    r.put(r#"{ "ConfigVersion": 2, "LoadingMode": "Fast" }"#);
    let loaded = store.load(None).unwrap();
    assert_eq!(loaded.journal_durability, JournalDurability::Fast);
    assert_eq!(store.current().journal_durability, JournalDurability::Fast);
}

// C#: JournalDurabilityTests.JournalDurability_ParsesLeniently (three InlineData rows)
#[test]
fn journal_durability_parses_leniently() {
    for (text, expected) in [
        ("PowerLossSafe", JournalDurability::PowerLossSafe),
        ("powerlosssafe", JournalDurability::PowerLossSafe),
        ("garbage", JournalDurability::Fast),
    ] {
        let r = Rig::new();
        let store = r.store();
        r.put(&format!("{{ \"JournalDurability\": \"{text}\" }}"));
        assert_eq!(store.load(None).unwrap().journal_durability, expected, "text {text:?}");
    }
}

// C#: JournalDurabilityTests.Settings_RoundTrip_PreservesPowerLossSafe_AndProviderReadsLive
#[test]
fn settings_round_trip_preserves_power_loss_safe_and_provider_reads_live() {
    let r = Rig::new();
    let store = r.store();
    let provider_store = Arc::clone(&store);
    let journal = OperationJournal::with_options(
        &r.paths,
        Arc::clone(&r.fs) as Arc<dyn FileSystem>,
        Arc::clone(&r.clock) as Arc<dyn Clock>,
        OperationJournalOptions {
            durability: Some(Arc::new(move || provider_store.current().journal_durability)),
            ..OperationJournalOptions::default()
        },
    );
    assert_eq!(journal.durability(), JournalDurability::Fast);

    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    settings.journal_durability = JournalDurability::PowerLossSafe;
    store.save(settings).unwrap();
    let text = String::from_utf8(r.fs.file_content(&r.paths.config_file).expect("config.json written")).unwrap();
    assert!(text.contains("PowerLossSafe"), "{text}");

    let reloaded = r.store();
    assert_eq!(
        reloaded.load(None).unwrap().journal_durability,
        JournalDurability::PowerLossSafe
    );
    assert_eq!(journal.durability(), JournalDurability::PowerLossSafe); // live, no restart
}
