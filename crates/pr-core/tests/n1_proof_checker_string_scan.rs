//! N1 run-proof (mutation close-out 2026-11-05) for the `checker.rs:711:41` survivor `self.pos + 4` -> `self.pos * 4` in
//! `Scanner::string` (`let end = (self.pos + 4).min(len)`; Q8: "equivalent by reading"). The two expressions differ only for
//! `pos` 0 or 1 (for `pos >= 2`, `pos * 4 >= pos + 4`, and the extra bytes are cut by `valid_up_to`, the first scalar is
//! complete in both). `string` is entered only after a consumed opening quote that follows the root `{`, so `pos >= 2`.
//! This file turns that into a run over a finite domain through the public `UpdateChecker::interpret`:
//!
//! * bodies `{"tag_name":"v9.9.9","k<K>":"<V>"}` for every K, V of up to 3 / 2 and 2 / 3 characters over ten units that include
//!   one-, two-, three- and four-byte characters, a backslash, `n`, `u`, `0`, a bare quote and a control character; the expected
//!   result is an independent oracle: when `serde_json` accepts the body as a JSON object the answer of the same body without
//!   the extra member, otherwise `BadResponse`;
//! * every body of up to 4 characters over `" a é { } : \ space` (these start strings at offsets 0 and 1): none has a
//!   `tag_name`, so each must be `BadResponse`.
//!
//! The test is meant to PASS with the original code and with the hand-applied mutant (proof of equivalence, not a killer).

use pr_core::updates::checker::UpdateChecker;
use pr_core::updates::version::{AppVersion, UpdateCheckResult, UpdateFailure};

fn current() -> AppVersion {
    AppVersion {
        major: 1,
        minor: 0,
        patch: 0,
        is_prerelease: false,
    }
}

fn all_strings(units: &[&str], max_len: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut frontier = vec![String::new()];
    for _ in 0..max_len {
        let mut next = Vec::new();
        for prefix in &frontier {
            for unit in units {
                next.push(format!("{prefix}{unit}"));
            }
        }
        out.extend(next.iter().cloned());
        frontier = next;
    }
    out
}

#[test]
fn members_with_multibyte_keys_and_values_match_a_serde_json_oracle() {
    let units = [
        "a",
        "\u{e9}",
        "\u{20ac}",
        "\u{1f600}",
        "\\",
        "n",
        "u",
        "0",
        "\"",
        "\u{1}",
    ];
    let base_body = r#"{"tag_name":"v9.9.9"}"#;
    let base = UpdateChecker::interpret(base_body, current());
    assert_ne!(
        base.failure(),
        UpdateFailure::BadResponse,
        "the base body is a valid release"
    );
    let bad = UpdateCheckResult::failed(UpdateFailure::BadResponse);
    let keys_long = all_strings(&units, 3);
    let keys_short = all_strings(&units, 2);
    let values_long = all_strings(&units, 3);
    let values_short = all_strings(&units, 2);
    let mut accepted = 0usize;
    let mut total = 0usize;
    for (keys, values) in [(&keys_long, &values_short), (&keys_short, &values_long)] {
        for k in keys {
            for v in values {
                let body = format!("{{\"tag_name\":\"v9.9.9\",\"k{k}\":\"{v}\"}}");
                let valid = matches!(
                    serde_json::from_str::<serde_json::Value>(&body),
                    Ok(serde_json::Value::Object(_))
                );
                let expected = if valid { &base } else { &bad };
                assert_eq!(&UpdateChecker::interpret(&body, current()), expected, "body {body:?}");
                accepted += usize::from(valid);
                total += 1;
            }
        }
    }
    assert!(total > 200_000, "total {total}");
    assert!(
        accepted > 20_000,
        "valid bodies exist, so the oracle is not vacuous: {accepted}"
    );
}

#[test]
fn bodies_whose_strings_start_at_offsets_0_and_1_are_bad_responses() {
    let units = ["\"", "a", "\u{e9}", "{", "}", ":", "\\", " "];
    let mut checked = 0;
    for body in all_strings(&units, 4) {
        assert_eq!(
            UpdateChecker::interpret(&body, current()).failure(),
            UpdateFailure::BadResponse,
            "body {body:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 1 + 8 + 64 + 512 + 4096);
}
