//! Mutation killers for the routing/settings debts (MUT-routing-startup-rest, see docs/reviews/MUTATION-ROUTING-STARTUP-REST.md).
use std::sync::Arc;

use pr_core::abstractions::KeyNameValidator;
use pr_core::settings::app_settings::EnglishSettingsTexts;
use pr_core::settings::normalizer::SettingsValidator;

struct AcceptAll;
impl KeyNameValidator for AcceptAll {
    fn is_valid_key_name(&self, _key_name: &str) -> bool {
        true
    }
}

#[test]
fn settings_validator_debug_names_the_type() {
    // Mutant: normalizer.rs `<impl fmt::Debug for SettingsValidator>::fmt -> Ok(Default::default())` (writes nothing).
    let validator = SettingsValidator::new(Arc::new(AcceptAll), Arc::new(EnglishSettingsTexts));
    assert!(format!("{validator:?}").starts_with("SettingsValidator"));
}
