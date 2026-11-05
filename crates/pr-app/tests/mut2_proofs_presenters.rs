//! N3 run-proofs for the mutants of `settings_field_map.rs`, `recovery_presentation.rs` and `scrollbar_geometry.rs` that
//! M4 only argued equivalent by reading the code. Each test is a differential or exhaustive test over the whole input
//! domain of the mutated expression (or, where the domain is the f64 line, over every special value class plus a
//! deterministic pseudo-random sample). It passes on the unmutated code AND under the mutant named in its comment, so the
//! mutant is equivalent on that domain by a run. Hand-written, no golden data, no production change.
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::presenters::recovery_presentation::{RecoveryTexts, RecoveryTimeFormatter};
use pr_app::presenters::settings_field_map::{FieldContext, bindings, dotnet_double_text, try_double};
use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_app::viewing::scrollbar_geometry::{
    ScrollAxis, ScrollBarBox, ScrollBarLayout, ScrollBarMetrics, compute, inside, offset_for_thumb_start,
};
use pr_app::viewport::types::ViewportLayout;
use pr_core::model::enums::{ImageSortMode, MiddleClickAction};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings, canonicalize};
use pr_core::time::DateTimeUtc;
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::NumberCulture;

/// xorshift64*, fixed seeds: the samples are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Every class of f64 the geometry code distinguishes: NaN, both infinities, huge, negative, both zeros, a subnormal, fractions.
const V: [f64; 13] = [
    f64::NAN,
    f64::NEG_INFINITY,
    -1e300,
    -1.0,
    -0.5,
    -0.0,
    0.0,
    5e-324,
    0.5,
    1.0,
    2.0,
    1e300,
    f64::INFINITY,
];

// --- settings_field_map.rs: try_double (mutants 256, 257, 258, 260:14, 262:90) -----------------------------------------------

/// Independent model of `try_double`: trim .NET whitespace; a comma refuses; `inf` (any case, no sign) refuses; `infinity`
/// (any case, optional sign) is the matching infinity; everything else is `str::parse::<f64>` (which also reads every case of `nan`
/// with an optional sign as NaN). Written without the arm structure of the production code.
fn try_double_model(text: &str) -> Option<f64> {
    let t = text.trim_matches(|c: char| matches!(c, '\u{9}'..='\u{d}' | ' '));
    if t.contains(',') {
        return None;
    }
    if t.eq_ignore_ascii_case("inf") {
        return None;
    }
    let (negative, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if body.eq_ignore_ascii_case("infinity") {
        return Some(if negative { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    t.parse::<f64>().ok()
}

fn same_option_f64(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits(),
        _ => false,
    }
}

/// Every ASCII case variant of `word`.
fn case_variants(word: &str) -> Vec<String> {
    let letters: Vec<char> = word.chars().collect();
    (0..1u32 << letters.len())
        .map(|mask| {
            letters
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    if mask >> i & 1 == 1 {
                        c.to_ascii_uppercase()
                    } else {
                        c.to_ascii_lowercase()
                    }
                })
                .collect()
        })
        .collect()
}

#[test]
fn try_double_equals_its_model_on_every_case_variant_sign_padding_and_comma_of_nan_inf_and_infinity() {
    // Mutants 256 (delete the NaN arm), 257 and 258 (delete the "Infinity" arms), 260:14 (guard of the infinity arm to false)
    // and 262:90 (`||` to `&&` before `contains(',')`): each only changes the path, never the value, so the model, which has no
    // such arms, must agree with the function on this complete domain: ALL 8 case variants of nan and inf, ALL 256 of infinity.
    let mut words: Vec<String> = Vec::new();
    for w in ["nan", "inf", "infinity"] {
        words.extend(case_variants(w));
    }
    words.extend(
        [
            "",
            "1",
            "-1",
            "+1",
            "1.5",
            "-0",
            "+0.0",
            "1e5",
            "1E-5",
            "1e400",
            "-1e400",
            ".5",
            "5.",
            ".",
            "+",
            "-",
            "e5",
            "1e",
            "0x10",
            "1_0",
            "1 2",
            "1,5",
            "1,",
            ",",
            "١٢",
            "infinit",
            "infinityy",
            "in f",
            "n an",
            "-.5e+3",
        ]
        .map(str::to_owned),
    );
    let signs = ["", "+", "-", "--", "+-"];
    let pads = ["", " ", "\t", "\n", "\u{b}\u{c}\r ", "\u{a0}", "\u{2003}"];
    let commas = ["", ",", "1,", ",1"];
    let mut checked = 0usize;
    let mut infinity_variants = 0usize;
    for word in &words {
        for sign in signs {
            for pad in pads {
                for comma in commas {
                    for text in [
                        format!("{pad}{sign}{word}{pad}"),
                        format!("{pad}{sign}{word}{comma}{pad}"),
                        format!("{pad}{comma}{sign}{word}"),
                    ] {
                        let actual = try_double(&text);
                        let expected = try_double_model(&text);
                        assert!(
                            same_option_f64(actual, expected),
                            "{text:?}: {actual:?} vs model {expected:?}"
                        );
                        checked += 1;
                        if word.len() == 8 && word.eq_ignore_ascii_case("infinity") {
                            infinity_variants += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 100_000, "{checked}");
    assert!(
        infinity_variants > 100_000,
        "all 256 case variants of infinity were reached"
    );
    // The documented values of the arms themselves, so the model is anchored on something other than itself.
    assert!(try_double("NaN").is_some_and(f64::is_nan));
    assert_eq!(try_double("Infinity"), Some(f64::INFINITY));
    assert_eq!(try_double("+Infinity"), Some(f64::INFINITY));
    assert_eq!(try_double("-Infinity"), Some(f64::NEG_INFINITY));
    assert_eq!(try_double("-INFINITY"), Some(f64::NEG_INFINITY));
    assert_eq!(try_double("inf"), None);
    assert_eq!(try_double("INF"), None);
    assert_eq!(try_double("1,5"), None);
}

// --- settings_field_map.rs: dotnet_double_text (mutants 289:12 and 289:14) ----------------------------------------------------

/// The function without the `split_once('e')` branch (the proposed cleanup): the `else` branch and the tail only.
fn dotnet_double_text_without_e_branch(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f64::INFINITY {
        return "Infinity".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let text = value.to_string();
    if value.abs() < 1.0e-4 || value.abs() >= 1.0e17 {
        let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
        let digits = format!("{whole}{frac}");
        let first = digits.trim_start_matches('0').chars().next().unwrap_or('0');
        let exp = if whole == "0" {
            -(frac.chars().take_while(|c| *c == '0').count() as i32) - 1
        } else {
            whole.len() as i32 - 1
        };
        return format!("{first}E{exp:+03}");
    }
    text
}

/// Frozen copy of `dotnet_double_text` as it was BEFORE the dead `split_once('e')` branch was removed (P-393 option 1). The
/// production function must stay byte-for-byte equal to it on every input.
fn dotnet_double_text_original(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f64::INFINITY {
        return "Infinity".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let text = value.to_string();
    if let Some((mantissa, exponent)) = text.split_once('e') {
        let exponent = exponent.parse::<i32>().unwrap_or(0);
        if !(-4..17).contains(&exponent) {
            return format!("{mantissa}E{exponent:+03}");
        }
    } else if value.abs() < 1.0e-4 || value.abs() >= 1.0e17 {
        let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
        let digits = format!("{whole}{frac}");
        let first = digits.trim_start_matches('0').chars().next().unwrap_or('0');
        let exp = if whole == "0" {
            -(frac.chars().take_while(|c| *c == '0').count() as i32) - 1
        } else {
            whole.len() as i32 - 1
        };
        return format!("{first}E{exp:+03}");
    }
    text
}

#[test]
fn dotnet_double_text_never_takes_the_exponent_branch_and_equals_the_function_without_it() {
    // Mutants 289:12 (delete `!`) and 289:14 (delete `-`) lived only inside `if let Some(..) = text.split_once('e')` (now removed). Domain: every
    // finite f64 class: ALL 2046 normal biased exponents and the subnormal one, times a spread of mantissas, both signs, plus
    // 400k pseudo-random bit patterns. For each, `to_string()` has no `e` (so the branch is dead) and the function equals the
    // function with the branch removed.
    let mantissas: Vec<u64> = {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut m = vec![
            0,
            1,
            2,
            0x8_0000_0000_0000,
            0xF_FFFF_FFFF_FFFF,
            0x5_5555_5555_5555,
            0xA_AAAA_AAAA_AAAA,
        ];
        m.extend((0..40).map(|_| rng.next() & 0xF_FFFF_FFFF_FFFF));
        m
    };
    let mut seen = 0usize;
    let mut check = |value: f64| {
        if !value.is_finite() {
            return;
        }
        let text = value.to_string();
        assert!(!text.contains('e'), "{value:e} displays as {text}");
        assert_eq!(
            dotnet_double_text(value),
            dotnet_double_text_without_e_branch(value),
            "{value:e}"
        );
        assert_eq!(
            dotnet_double_text(value),
            dotnet_double_text_original(value),
            "{value:e}"
        );
        seen += 1;
    };
    for biased in 0u64..2047 {
        for &mantissa in &mantissas {
            for sign in [0u64, 1] {
                check(f64::from_bits(sign << 63 | biased << 52 | mantissa));
            }
        }
    }
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for _ in 0..400_000 {
        check(f64::from_bits(rng.next()));
    }
    for exponent in -330i32..=330 {
        for digit in 1..=9 {
            let v = f64::from(digit) * 10f64.powi(exponent);
            check(v);
            check(-v);
            check(f64::from_bits(v.to_bits().wrapping_add(1)));
            check(f64::from_bits(v.to_bits().wrapping_sub(1)));
        }
    }
    assert!(seen > 300_000, "{seen}");
    // Anchors on the thresholds that the function applies.
    assert_eq!(dotnet_double_text(1.5), "1.5");
    assert_eq!(dotnet_double_text(f64::NAN), "NaN");
}

// --- settings_field_map.rs: apply_actions (mutant 684:76) ---------------------------------------------------------------------

#[test]
fn no_shortcut_property_name_is_longer_than_128_chars_so_the_known_name_test_is_moot_for_long_keys() {
    // 684:76 changes `canonicalize(name) == key || key.eq_ignore_ascii_case(name)` to `&&`, inside `.any(..)`, whose result only
    // matters through `!any && key.len() > 128`. For a key longer than 128 chars `any` is false in both forms as soon as no name
    // (nor its canonical form) is that long: `==` and `eq_ignore_ascii_case` need equal lengths. Checked for ALL names.
    for name in ShortcutMappings::PROPERTY_NAMES {
        assert!(name.len() <= 128, "{name}");
        assert!(canonicalize(Some(name)).len() <= 128, "{name}");
    }
    // The predicate in both forms over every key shape: all names, their canonical forms, case variants, and long keys.
    let mut keys: Vec<String> = Vec::new();
    for name in ShortcutMappings::PROPERTY_NAMES {
        keys.push(name.to_owned());
        keys.push(name.to_ascii_lowercase());
        keys.push(name.to_ascii_uppercase());
        keys.push(canonicalize(Some(name)));
        keys.push(format!("{name}{}", "k".repeat(129)));
        keys.push(name.repeat(40));
    }
    for len in [0, 1, 127, 128, 129, 130, 256, 1000] {
        keys.push("k".repeat(len));
        keys.push("K".repeat(len));
    }
    let any = |key: &str, and: bool| {
        ShortcutMappings::PROPERTY_NAMES.iter().any(|name| {
            let a = canonicalize(Some(name)) == key;
            let b = key.eq_ignore_ascii_case(name);
            if and { a && b } else { a || b }
        })
    };
    for key in &keys {
        let refused = |and: bool| !any(key, and) && key.len() > 128;
        assert_eq!(refused(false), refused(true), "{key:?}");
        if key.len() > 128 {
            assert!(!any(key, false) && !any(key, true), "{key:?}");
        }
    }
    // And through the real row: long keys are refused, every property name is accepted as a shortcut.
    let row = bindings()
        .iter()
        .find(|b| b.properties.contains(&"Actions"))
        .expect("Actions row");
    let texts = built_in_english_localizer();
    let numbers = NumberCulture::invariant();
    let ctx = FieldContext {
        texts: &texts,
        numbers: &numbers,
    };
    let run = |shortcut: &str| {
        let form = SettingsFormState {
            actions_json: format!("[{{\"Name\":\"N\",\"Shortcut\":\"{shortcut}\"}}]"),
            ..SettingsFormState::default()
        };
        let mut target = AppSettings::new(&EnglishSettingsTexts);
        target.actions = vec![ReviewAction::new("Before")];
        (row.apply)(&form, &mut target, &ctx).is_none()
    };
    for name in ShortcutMappings::PROPERTY_NAMES {
        assert!(run(name), "{name}");
        assert!(run(&name.to_ascii_lowercase()), "{name}");
        assert!(!run(&format!("{name}{}", "k".repeat(129))), "{name} + 129 chars");
    }
    for len in [127, 128] {
        assert!(run(&"k".repeat(len)), "{len}");
    }
    for len in [129, 130, 400] {
        assert!(!run(&"k".repeat(len)), "{len}");
    }
}

// --- settings_field_map.rs: apply_middle / apply_sort (mutants 735, 748, 754) -----------------------------------------------------

fn apply_row(property: &str, edit: impl FnOnce(&mut SettingsFormState), target: &mut AppSettings) {
    let mut form = SettingsFormState::default();
    edit(&mut form);
    let texts = built_in_english_localizer();
    let numbers = NumberCulture::invariant();
    let ctx = FieldContext {
        texts: &texts,
        numbers: &numbers,
    };
    let row = bindings()
        .iter()
        .find(|b| b.properties.contains(&property))
        .unwrap_or_else(|| panic!("no row for {property}"));
    assert_eq!((row.apply)(&form, target, &ctx), None, "{property} never refuses");
}

#[test]
fn middle_click_row_maps_a_wide_index_range_like_the_table_with_actual_size_for_everything_else() {
    // 735: deleting the `2 =>` arm sends 2 to the `_ => ActualSize` arm, the same value. Domain: -3000..=3000 and the i32 extremes.
    let model = |index: i32| match index {
        0 => MiddleClickAction::None,
        1 => MiddleClickAction::ClickZoom,
        3 => MiddleClickAction::Fit,
        4 => MiddleClickAction::FitWidth,
        5 => MiddleClickAction::FitWidth2,
        _ => MiddleClickAction::ActualSize,
    };
    for index in (-3000..=3000).chain([i32::MIN, i32::MIN + 1, i32::MAX - 1, i32::MAX]) {
        let mut target = AppSettings::new(&EnglishSettingsTexts);
        target.middle_click_action = Some(MiddleClickAction::None);
        apply_row(
            "MiddleClickAction",
            |f| f.middle_click_action_index = index,
            &mut target,
        );
        assert_eq!(target.middle_click_action, Some(model(index)), "{index}");
    }
}

#[test]
fn sort_row_maps_every_known_tag_and_a_wide_set_of_other_text_like_the_table_with_name_for_everything_else() {
    // 748 (delete `Some("Name")`) and 754 (delete `Some("0")`) fall to `_ => Name`, the same value.
    let model = |tag: Option<&str>| match tag.map(str::trim) {
        Some("SizeDescending" | "1") => ImageSortMode::SizeDescending,
        Some("SizeAscending" | "2") => ImageSortMode::SizeAscending,
        Some("Default" | "3") => ImageSortMode::Default,
        Some("NameAscending" | "4") => ImageSortMode::NameAscending,
        Some("NameDescending" | "5") => ImageSortMode::NameDescending,
        _ => ImageSortMode::Name,
    };
    let mut tags: Vec<Option<String>> = vec![None];
    let known = [
        "Name",
        "SizeDescending",
        "SizeAscending",
        "Default",
        "NameAscending",
        "NameDescending",
        "0",
        "1",
        "2",
        "3",
        "4",
        "5",
    ];
    for tag in known {
        for pad in ["", " ", "\t", "\n  "] {
            tags.push(Some(format!("{pad}{tag}{pad}")));
        }
        tags.push(Some(tag.to_ascii_lowercase()));
        tags.push(Some(tag.to_ascii_uppercase()));
        tags.push(Some(format!("{tag}x")));
    }
    for other in [
        "",
        " ",
        "6",
        "-1",
        "00",
        "01",
        "Nam",
        "name ",
        "Size",
        "x",
        "NaMe",
        "Name\u{a0}",
        "\u{feff}Name",
    ] {
        tags.push(Some(other.to_owned()));
    }
    for tag in &tags {
        let mut target = AppSettings::new(&EnglishSettingsTexts);
        target.image_sort_mode = Some(ImageSortMode::SizeAscending);
        apply_row("ImageSortMode", |f| f.sort_mode_tag = tag.clone(), &mut target);
        assert_eq!(target.image_sort_mode, Some(model(tag.as_deref())), "{tag:?}");
    }
}

// --- recovery_presentation.rs: format_size (mutant 132:55) ---------------------------------------------------------------------

struct FakeTime;

impl RecoveryTimeFormatter for FakeTime {
    fn format_local_general(&self, utc: DateTimeUtc) -> String {
        format!("<{}>", utc.ticks())
    }
}

/// Digit grouping written from the right, the way a person does it.
fn grouped(bytes: i64, separator: &str) -> String {
    let digits: Vec<char> = bytes.unsigned_abs().to_string().chars().collect();
    let mut out = String::new();
    for (i, ch) in digits.iter().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.insert_str(0, separator);
        }
        out.insert(0, *ch);
    }
    if bytes < 0 {
        out.insert(0, '-');
    }
    out
}

#[test]
fn format_size_equals_right_to_left_grouping_for_every_digit_count_and_residue() {
    // `index > first` to `>=` only differs for `index == first`, which `index == first` already accepts. The output depends only on
    // the digit count and the sign, so ALL 19 digit counts of both signs are covered by 10^k - 1, 10^k and 10^k + 1, plus the i64
    // extremes and 300k pseudo-random values.
    let loc = built_in_english_localizer();
    let mut values: Vec<i64> = vec![0, 1, -1, i64::MAX, i64::MIN, i64::MIN + 1, i64::MAX - 1];
    let mut power = 1i64;
    for _ in 0..19 {
        for v in [power - 1, power, power + 1, power * 9, power * 9 + 8] {
            values.push(v);
            values.push(-v);
        }
        match power.checked_mul(10) {
            Some(next) => power = next,
            None => break,
        }
    }
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    values.extend((0..300_000).map(|_| (rng.next() >> (rng.next() % 64)) as i64));
    for separator in [",", " ", "'", ""] {
        let t = RecoveryTexts {
            loc: &loc,
            time: &FakeTime,
            group_separator: separator,
        };
        for &v in &values {
            assert_eq!(
                t.format_size(v),
                format!("{} bytes", grouped(v, separator)),
                "{v} sep {separator:?}"
            );
        }
    }
}

// --- scrollbar_geometry.rs: inside (mutants 345:16 and 346:24) -------------------------------------------------------------------

fn inside_model(rect: RectD, p: PointD) -> bool {
    use std::cmp::Ordering::Greater;
    rect.width.partial_cmp(&0.0) == Some(Greater)
        && rect.height.partial_cmp(&0.0) == Some(Greater)
        && p.x >= rect.x
        && p.x < rect.x + rect.width
        && p.y >= rect.y
        && p.y < rect.y + rect.height
}

#[test]
fn inside_equals_the_strict_positive_size_model_on_every_class_of_rect_and_point() {
    // `width > 0` / `height > 0` to `>=` differ only for a zero size, where `x >= rect.x && x < rect.x + 0` cannot hold. Domain: the
    // full cartesian product of 13 value classes for x, y, width, height of the rect and x, y of the point (4.8 million cases).
    let mut true_cases = 0usize;
    for &rx in &V {
        for &ry in &V {
            for &rw in &V {
                for &rh in &V {
                    let rect = RectD {
                        x: rx,
                        y: ry,
                        width: rw,
                        height: rh,
                    };
                    for &px in &V {
                        for &py in &V {
                            let point = PointD { x: px, y: py };
                            let actual = inside(rect, point);
                            assert_eq!(actual, inside_model(rect, point), "{rect:?} {point:?}");
                            true_cases += usize::from(actual);
                        }
                    }
                }
            }
        }
    }
    assert!(true_cases > 1000, "{true_cases}");
}

// --- scrollbar_geometry.rs: cs_min (mutants 357:17, 357:27, 357:32) ---------------------------------------------------------------

/// FNV-1a over u64 words.
struct Digest(u64);

impl Digest {
    fn new() -> Self {
        Self(0xCBF2_9CE4_8422_2325)
    }
    fn word(&mut self, w: u64) {
        for byte in w.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    /// Bit-exact (the sign of a zero is part of the digest), all NaNs as one.
    fn float(&mut self, v: f64) {
        self.word(if v.is_nan() { 0x7FF8_0000_0000_0000 } else { v.to_bits() });
    }
    fn rect(&mut self, r: RectD) {
        for v in [r.x, r.y, r.width, r.height] {
            self.float(v);
        }
    }
    fn scroll_box(&mut self, b: &ScrollBarBox) {
        self.word(b.axis as u64);
        self.word(u64::from(b.visible));
        self.rect(b.bar);
        self.rect(b.track);
        self.rect(b.thumb);
        self.word(u64::from(b.thumb_visible));
        self.float(b.max_offset);
        self.float(b.offset);
        self.float(b.viewport_length);
    }
    fn layout(&mut self, l: &ScrollBarLayout) {
        self.scroll_box(&l.vertical);
        self.scroll_box(&l.horizontal);
        self.rect(l.corner);
        self.word(u64::from(l.has_corner));
    }
}

fn viewport(w: f64, h: f64, vv: bool, hv: bool, max_h: f64, max_v: f64) -> ViewportLayout {
    ViewportLayout {
        viewport_width: w,
        viewport_height: h,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: hv,
        vertical_bar_visible: vv,
        image_rect: RectD::default(),
        max_horizontal_offset: max_h,
        max_vertical_offset: max_v,
    }
}

// The pinned digests were produced by the UNMUTATED code. Under a mutant of `cs_min` the same inputs must give the same bits:
// a different digest would mean the mutant is observable (and the test would then be its killer).
const DIGEST_COMPUTE_CLIENT_THICKNESS: u64 = 0xf8b0577b79ec8ed0;
const DIGEST_OFFSET_FOR_THUMB_START: u64 = 0xad00f71f939dd291;
const DIGEST_BUILD_BOX_GRID: u64 = 0x4d2c6cbb933d6a86;
const DIGEST_BUILD_BOX_SAMPLE: u64 = 0xaf4521e0e6328d72;

#[test]
fn compute_client_size_and_thickness_grid_is_bit_identical_to_the_unmutated_digest() {
    // Lines 234/235: `cs_min(thickness, width)` and `cs_min(thickness, height)`. Domain: 13 x 13 x 13 client sizes and
    // thicknesses, all four bar visibility combinations (35k layouts, every output bit hashed).
    let mut d = Digest::new();
    for &w in &V {
        for &h in &V {
            for &t in &V {
                for (vv, hv) in [(false, false), (true, false), (false, true), (true, true)] {
                    let metrics = ScrollBarMetrics {
                        thickness: t,
                        ..ScrollBarMetrics::default()
                    };
                    let l = compute(
                        SizeD { width: w, height: h },
                        &viewport(w, h, vv, hv, 50.0, 70.0),
                        3.0,
                        4.0,
                        &metrics,
                    );
                    d.layout(&l);
                }
            }
        }
    }
    assert_eq!(d.0, DIGEST_COMPUTE_CLIENT_THICKNESS, "digest {:#018x}", d.0);
}

#[test]
fn offset_for_thumb_start_grid_is_bit_identical_to_the_unmutated_digest() {
    // Line 313: `cs_min(b.max_offset, cs_max(0.0, ratio * b.max_offset))`. Domain: both axes, track start, track length, thumb length,
    // max offset and thumb start each over the 13 value classes (742k calls).
    let mut d = Digest::new();
    for axis in [ScrollAxis::Vertical, ScrollAxis::Horizontal] {
        for &start in &V {
            for &track in &V {
                for &thumb in &V {
                    for &max_offset in &V {
                        let b = ScrollBarBox {
                            axis,
                            visible: true,
                            bar: RectD::default(),
                            track: RectD {
                                x: start,
                                y: start,
                                width: track,
                                height: track,
                            },
                            thumb: RectD {
                                x: 0.0,
                                y: 0.0,
                                width: thumb,
                                height: thumb,
                            },
                            thumb_visible: true,
                            max_offset,
                            offset: 0.0,
                            viewport_length: 10.0,
                        };
                        for &thumb_start in &V {
                            d.float(offset_for_thumb_start(&b, thumb_start));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(d.0, DIGEST_OFFSET_FOR_THUMB_START, "digest {:#018x}", d.0);
}

#[test]
fn build_box_interaction_grid_is_bit_identical_to_the_unmutated_digest() {
    // Lines 415, 423, 441 (through `compute`): minimum thumb length, viewport length, max offset and offset each over the 13 value
    // classes, margins along and across over 5, two client sizes (1.4 million boxes), both bars.
    let margins = [-0.0, 0.0, 2.0, -3.0, f64::NAN];
    let mut d = Digest::new();
    for (cw, ch) in [(100.0, 100.0), (25.0, 300.0)] {
        for &min_thumb in &V {
            for &vp in &V {
                for &max in &V {
                    for &offset in &V {
                        for &along in &margins {
                            for &across in &[0.0, 1.0, f64::NAN] {
                                let metrics = ScrollBarMetrics {
                                    thickness: 10.0,
                                    track_margin_along: along,
                                    track_margin_across: across,
                                    min_thumb_length: min_thumb,
                                    thumb_radius: 4.0,
                                };
                                let l = compute(
                                    SizeD { width: cw, height: ch },
                                    &viewport(vp, vp, true, true, max, max),
                                    offset,
                                    offset,
                                    &metrics,
                                );
                                d.layout(&l);
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(d.0, DIGEST_BUILD_BOX_GRID, "digest {:#018x}", d.0);
}

#[test]
fn build_box_random_sample_is_bit_identical_to_the_unmutated_digest() {
    // The same code with every input drawn independently from the value classes and from random finite doubles (1 million layouts).
    let mut rng = Rng(0x0DDB_1A5E_5BAD_5EED);
    let pick = |rng: &mut Rng| -> f64 {
        let r = rng.next();
        if r % 4 == 0 {
            f64::from_bits(rng.next())
        } else {
            V[(r >> 8) as usize % V.len()]
        }
    };
    let mut d = Digest::new();
    for _ in 0..1_000_000 {
        let (w, h) = (pick(&mut rng), pick(&mut rng));
        let metrics = ScrollBarMetrics {
            thickness: pick(&mut rng),
            track_margin_along: pick(&mut rng),
            track_margin_across: pick(&mut rng),
            min_thumb_length: pick(&mut rng),
            thumb_radius: pick(&mut rng),
        };
        let flags = rng.next();
        let l = compute(
            SizeD { width: w, height: h },
            &viewport(
                pick(&mut rng),
                pick(&mut rng),
                flags & 1 == 1,
                flags & 2 == 2,
                pick(&mut rng),
                pick(&mut rng),
            ),
            pick(&mut rng),
            pick(&mut rng),
            &metrics,
        );
        d.layout(&l);
        for axis in [&l.vertical, &l.horizontal] {
            d.float(offset_for_thumb_start(axis, pick(&mut rng)));
        }
    }
    assert_eq!(d.0, DIGEST_BUILD_BOX_SAMPLE, "digest {:#018x}", d.0);
}
