//! Port of `LocalizationGeneratorTests` (C# baseline d92d6aae) over `keygen::plan` / `render_*`. The Roslyn compile
//! tests cannot be ported; "the generated code compiles" is proven by building `pr-l10n` itself with the shipped
//! `en.json` (see `generated_tr.rs`), and the rendered text is asserted here with exact strings.
//! Each test names its C# twin in a comment.

#[path = "../build/keygen.rs"]
#[allow(dead_code)] // the whole generator module is compiled here; the tests use part of it
mod keygen;

use keygen::{CatalogInput, MemberKind, Plan, plan};
use pr_l10n::localizer::built_in_english;
use pr_l10n::template::LocTemplate;

fn input(path: &str, json: &str) -> CatalogInput {
    CatalogInput {
        path: path.to_owned(),
        content: Some(json.to_owned()),
    }
}

fn run(json: &str) -> Plan {
    plan(&[input(r"C:\x\Languages\en.json", json)])
}

fn catalog(entries: &[&str]) -> String {
    format!("{{ \"_meta\": {{\"code\":\"en\"}}, {} }}", entries.join(", "))
}

fn ids(plan: &Plan) -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = plan.diagnostics.iter().map(|d| d.id).collect();
    ids.sort_unstable();
    ids
}

struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn below(&mut self, n: usize) -> usize {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x % n as u64) as usize
    }
}

#[test]
fn shipped_catalog_generates_compilable_code() {
    // C#: LocalizationGeneratorTests.ShippedCatalog_GeneratesCompilableCode
    let json = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets")
            .join("languages")
            .join("en.json"),
    )
    .expect("assets/languages/en.json");

    let plan = run(&json);

    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    assert!(plan.generated);
    let keys = keygen::render_keys(&plan);
    for (key, _) in built_in_english().entries() {
        assert!(keys.contains(&format!("\"{key}\";")), "{key}");
    }
    // Every const name is a valid, unique Rust identifier and every member has a function.
    let mut names: Vec<String> = plan.consts.iter().map(|c| keygen::const_name(&c.ident)).collect();
    let count = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), count);
    let tr = keygen::render_tr(&plan);
    for member in &plan.members {
        assert!(
            tr.contains(&format!("pub fn {}(", keygen::fn_name(&member.ident))),
            "{}",
            member.ident
        );
    }
}

#[test]
fn broken_catalog_reports_diagnostic() {
    // C#: LocalizationGeneratorTests.BrokenCatalog_ReportsDiagnostic
    let cases: [(&str, Option<&str>); 19] = [
        ("{", Some("PRLOC001")),
        ("[]", Some("PRLOC001")),
        (r#"{"a": 1}"#, Some("PRLOC001")),
        (r#"{"a": "x",}"#, None),
        (r#"{"1abc": "x"}"#, Some("PRLOC002")),
        (r#"{"a..b": "x"}"#, Some("PRLOC002")),
        (r#"{"a.": "x"}"#, Some("PRLOC002")),
        (r#"{"a-b": "x"}"#, Some("PRLOC002")),
        (r#"{"é": "x"}"#, Some("PRLOC002")),
        (r#"{"": "x"}"#, Some("PRLOC002")),
        (r#"{"a.b": "x", "aB": "y"}"#, Some("PRLOC003")),
        (r#"{"tr": "x"}"#, Some("PRLOC003")),
        (r#"{"equals": "x"}"#, Some("PRLOC003")),
        (r#"{"a": "x", "a": "y"}"#, Some("PRLOC006")),
        (r#"{"a": "{oops"}"#, Some("PRLOC004")),
        (r#"{"a": "}"}"#, Some("PRLOC004")),
        (r#"{"a": "{1}"}"#, Some("PRLOC004")),
        (r#"{"a": "{ x}"}"#, Some("PRLOC004")),
        (r#"{"a.one": "x"}"#, Some("PRLOC005")),
    ];
    for (json, expected) in cases {
        let plan = run(json);

        match expected {
            None => assert!(plan.diagnostics.is_empty(), "{json}: {:?}", plan.diagnostics),
            Some(id) => assert!(ids(&plan).contains(&id), "{json}: {:?}", plan.diagnostics),
        }
        // A broken catalog never produces a generated flag together with an invalid-JSON diagnostic.
        if expected == Some("PRLOC001") {
            assert!(!plan.generated, "{json}");
        }
    }
}

#[test]
fn two_catalogs_reports_diagnostic_and_uses_first_by_path() {
    // C#: LocalizationGeneratorTests.TwoCatalogs_ReportsDiagnosticAndUsesFirstByPath (RV-P07)
    let result = plan(&[
        input(r"C:\z\Languages\en.json", &catalog(&["\"second\": \"x\""])),
        input(r"C:\a\Languages\en.json", &catalog(&["\"first\": \"x\""])),
    ]);

    assert_eq!(ids(&result), ["PRLOC007"]);
    assert!(result.diagnostics[0].message.contains(r"C:\z\Languages\en.json"));
    assert!(
        result.diagnostics[0]
            .message
            .contains(r"only 'C:\a\Languages\en.json' (first by path)")
    );
    assert!(result.generated);
    let tr = keygen::render_tr(&result);
    assert!(tr.contains("pub fn first("));
    assert!(!tr.contains("second"));
}

#[test]
fn identifier_collision_is_prloc003_and_first_claimant_kept() {
    // C#: LocalizationGeneratorTests.IdentifierCollision_IsPrloc003_AndFirstClaimantKept (RV-P07)
    for (json, identifier, kept_text) in [
        (r#"{"x": "plain", "x.one": "one", "x.other": "many"}"#, "X", "plain"),
        (r#"{"a.b": "first", "a.B": "second"}"#, "AB", "first"),
    ] {
        let result = run(json);

        assert!(ids(&result).contains(&"PRLOC003"), "{json}: {:?}", result.diagnostics);
        let claimed: Vec<_> = result.members.iter().filter(|m| m.ident == identifier).collect();
        assert_eq!(claimed.len(), 1, "{json}");
        assert_eq!(claimed[0].english[0], kept_text);
        let tr = keygen::render_tr(&result);
        assert!(tr.contains(&format!("/// English: \"{kept_text}\"")), "{json}");
        assert_eq!(
            tr.matches(&format!("pub fn {}(", keygen::fn_name(identifier))).count(),
            1,
            "{json}"
        );
    }
}

#[test]
fn identifier_collision_reports_both_tr_keys_and_tr_types() {
    // Contract §22: one colliding key reports twice, `TrKeys.` first, then `Tr.`.
    let result = run(r#"{"a.b": "first", "a.B": "second"}"#);

    let messages: Vec<&str> = result.diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "Key 'a.B' generates identifier 'TrKeys.AB', which is already used by 'a.b'",
            "Key 'a.B' generates identifier 'Tr.AB', which is already used by 'a.b'",
        ]
    );
}

#[test]
fn awkward_text_still_compiles() {
    // C#: LocalizationGeneratorTests.AwkwardText_StillCompiles. Rust: the plan has no diagnostic and the rendered
    // doc comments, parameters and arguments are exactly what the compiler must accept.
    let json = catalog(&[
        r#""k.type": "Use {type} and {match} and {self} and {Self} and {crate} and {async}""#,
        r#""k.quotes": "He said \"hi\" \\ back""#,
        r#""k.xml": "</summary> & <b> ]]> */ /* {{literal}}""#,
        r#""k.lines": "a\u2028b\u0085c\u2029d\r\ne\nf""#,
        r#""k.unicode": "\ud83d\ude00 \u65e5\u672c \u0645\u0631\u062d\u0628\u0627""#,
        r#""k.same": "{a} {a} {A} {a1}""#,
        r#""files.one": "{count} file {extra}""#,
        r#""files.other": "{count} files""#,
        r#""k.count": "{count}""#,
    ]);

    let result = run(&json);

    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let tr = keygen::render_tr(&result);
    assert!(tr.contains(
        "pub fn k_type(r#type: impl Into<LocValue>, r#match: impl Into<LocValue>, self_: impl Into<LocValue>, \
         Self_: impl Into<LocValue>, crate_: impl Into<LocValue>, r#async: impl Into<LocValue>) -> String {\n    \
         Localizer::current().format(crate::keys::K_TYPE, &[LocArg::new(\"type\", r#type), \
         LocArg::new(\"match\", r#match), LocArg::new(\"self\", self_), LocArg::new(\"Self\", Self_), \
         LocArg::new(\"crate\", crate_), LocArg::new(\"async\", r#async)])\n}\n"
    ));
    // The decoded text (quotes and one backslash) is shown as is: a `///` line needs no escaping.
    assert!(tr.contains("/// English: \"He said \"hi\" \\ back\"\n"));
    assert!(tr.contains("/// English: \"</summary> & <b> ]]> */ /* {{literal}}\"\n"));
    // U+2028, U+0085, U+2029 and the CR of CRLF: spaces / dropped; LF shown as a backslash and `n`.
    assert!(tr.contains("/// English: \"a b c d\\ne\\nf\"\n"), "{tr}");
    assert!(tr.contains(
        "pub fn k_same(a: impl Into<LocValue>, A: impl Into<LocValue>, a1: impl Into<LocValue>) -> String {"
    ));
    assert!(tr.contains(
        "pub fn files(count: i64, extra: impl Into<LocValue>) -> String {\n    \
         Localizer::current().format_plural(\"files\", count, &[LocArg::new(\"count\", count), \
         LocArg::new(\"extra\", extra)])\n}\n"
    ));
    assert!(tr.contains("/// English: \"{count} file {extra}\" / \"{count} files\"\n"));
    assert!(tr.contains(
        "pub fn k_count(count: impl Into<LocValue>) -> String {\n    \
         Localizer::current().format(crate::keys::K_COUNT, &[LocArg::new(\"count\", count)])\n}\n"
    ));
}

#[test]
fn other_file_ignored_and_large_catalog_plans() {
    // C#: LocalizationGeneratorTests.OtherFileIgnored_AndLargeCatalogCompiles (the file name filter is the caller's:
    // no input at all means nothing is generated and nothing is reported).
    let none = plan(&[]);
    assert!(!none.generated);
    assert!(none.diagnostics.is_empty());

    let entries: Vec<String> = (0..5000)
        .map(|i| format!("\"group{}.item{i}\": \"Text {{n}} {i}\"", i % 50))
        .collect();
    let refs: Vec<&str> = entries.iter().map(String::as_str).collect();

    let result = run(&catalog(&refs));

    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics.first());
    assert_eq!(result.consts.len(), 5000);
    assert_eq!(result.members.len(), 5000);
}

#[test]
fn placeholders_agree_with_runtime_template_parser() {
    // C#: LocalizationGeneratorTests.Placeholders_AgreeWithRuntimeTemplateParser
    let pieces = [
        "{", "}", "{{", "}}", "a", " ", "é", "{x}", "{name}", "{count}", "{1}", "{ x}", "{}", "{x y}", "{X}", "{ñ}",
        "日本", "{a1}",
    ];
    for seed in 1..=2 {
        let mut r = XorShift::new(seed);
        let texts: Vec<String> = (0..1500)
            .map(|_| {
                (0..r.below(8))
                    .map(|_| pieces[r.below(pieces.len())])
                    .collect::<String>()
            })
            .collect();
        let entries: Vec<String> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| format!("\"t.k{i}\": {}", json_string(t)))
            .collect();
        let refs: Vec<&str> = entries.iter().map(String::as_str).collect();

        let result = run(&catalog(&refs));

        let mut rejected = vec![false; texts.len()];
        for d in &result.diagnostics {
            assert_eq!(d.id, "PRLOC004", "{}", d.message);
            let n: usize = d.message["Text of 't.k".len()..]
                .split('\'')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            rejected[n] = true;
        }
        for (i, text) in texts.iter().enumerate() {
            let runtime = LocTemplate::try_parse(text);
            assert_eq!(runtime.is_some(), !rejected[i], "{text:?}");
            assert_eq!(
                runtime.is_some(),
                keygen::try_get_placeholders(text).is_some(),
                "{text:?}"
            );
        }
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[test]
fn placeholders_follow_the_runtime_grammar() {
    // Hand cases for `try_get_placeholders` (C# TryGetPlaceholders).
    let ok = |text: &str| keygen::try_get_placeholders(text).expect(text);
    assert_eq!(ok("Hello {name}, {name} {count}"), ["name", "count"]);
    assert_eq!(ok("{{literal}} {a1}"), ["a1"]);
    assert_eq!(ok(""), Vec::<String>::new());
    for bad in ["{", "}", "{x", "x}", "{1}", "{}", "{x y}", "{a{b}", "{ñ}", "{{{"] {
        assert!(keygen::try_get_placeholders(bad).is_none(), "{bad:?}");
    }
}

#[test]
fn rust_naming_rules() {
    // Contract §25 (decided in the stub; the golden is keyed by the C# identifiers).
    assert_eq!(keygen::to_identifier("status.batchDone"), "StatusBatchDone");
    assert_eq!(keygen::to_identifier("a.2b"), "A2b");
    assert_eq!(keygen::const_name("StatusBatchDone"), "STATUS_BATCH_DONE");
    assert_eq!(keygen::fn_name("StatusBatchDone"), "status_batch_done");
    assert_eq!(keygen::fn_name("Type"), "r#type");
    assert_eq!(keygen::fn_name("Match"), "r#match");
    assert_eq!(keygen::fn_name("Test"), "test");
    assert_eq!(keygen::param_name("count"), "count");
    assert_eq!(keygen::param_name("type"), "r#type");
    assert_eq!(keygen::param_name("self"), "self_");
    assert_eq!(keygen::param_name("Self"), "Self_");
    assert_eq!(keygen::param_name("crate"), "crate_");
    assert_eq!(keygen::param_name("super"), "super_");
    assert_eq!(keygen::param_name("Type"), "Type");
    assert_eq!(keygen::param_name("gen"), "r#gen");
    assert_eq!(keygen::param_name("try"), "r#try");
}

#[test]
fn rust_unnameable_functions_are_built_in_member_collisions() {
    // Contract §25: key `self` (ident `Self`) cannot be a function name; the const is still generated.
    let result = run(r#"{"self": "x", "crate": "y", "super": "z", "ok": "w"}"#);

    let messages: Vec<&str> = result.diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "Key 'self' generates identifier 'Tr.Self', which is already used by a built-in member",
            "Key 'crate' generates identifier 'Tr.Crate', which is already used by a built-in member",
            "Key 'super' generates identifier 'Tr.Super', which is already used by a built-in member",
        ]
    );
    assert_eq!(result.consts.len(), 4);
    assert_eq!(result.members.len(), 1);
}

#[test]
fn plan_member_kinds_and_parameters() {
    // C#: TrGenerator member construction (golden `simple-and-params-and-plural`, by hand).
    let result = run(&catalog(&[
        r#""test.sample": "Sample""#,
        r#""test.greeting": "Hello {name}""#,
        r#""test.items.one": "{count} item""#,
        r#""test.items.other": "{count} items {x}""#,
        r#""status.batchDone": "Done {count} of {total}""#,
    ]));

    assert!(result.diagnostics.is_empty());
    let members: Vec<(&str, &MemberKind, Vec<&str>)> = result
        .members
        .iter()
        .map(|m| (m.ident.as_str(), &m.kind, m.params.iter().map(String::as_str).collect()))
        .collect();
    assert_eq!(members.len(), 4);
    assert_eq!(members[2].0, "TestItems");
    assert_eq!(
        members[2].1,
        &MemberKind::Plural {
            base: "test.items".to_owned()
        }
    );
    assert_eq!(members[2].2, ["x"]);
    assert_eq!(members[3].2, ["count", "total"]);
    let keys = keygen::render_keys(&result);
    assert!(keys.contains("pub const TEST_ITEMS_ONE: &str = \"test.items.one\";\n"));
    assert!(keys.ends_with('\n'));
}

#[test]
fn flat_reader_error_messages() {
    // The messages the golden pins (contract §21/§28); the offsets are bytes.
    let message = |text: &str| keygen::read_flat_json(text).expect_err(text).message;
    assert_eq!(message("["), "expected '{'");
    assert_eq!(message(""), "unexpected end of file, expected '{'");
    assert_eq!(message("{} x"), "unexpected content after the root object");
    assert_eq!(message("{\"a\": \"x"), "unterminated string");
    assert_eq!(message("{ /* x"), "unterminated comment");
    assert_eq!(message("{\"a\": \"\u{1}\"}"), "control character in string (escape it)");
    assert_eq!(message("{\"a\": \"\\u12\"}"), "invalid \\u escape");
    assert_eq!(message("{\"a\": \"\\x\"}"), "invalid escape '\\x'");
    assert_eq!(message("{\"_a\": 01}"), "invalid number");
    assert_eq!(message("{\"_a\": ?}"), "expected a JSON value");
    assert_eq!(message("{\"a\": \"x\" \"b\": \"y\"}"), "expected ',' or '}'");
    assert_eq!(message("{\"_a\": [1 2]}"), "expected ',' or ']'");
    assert_eq!(message("{\"a\": 1}"), "value of 'a' must be a string");
    assert_eq!(message("{\"a\": \"\\ud800\"}"), "unpaired surrogate in string");
    assert_eq!(
        message(&format!("{{\"_a\": {}{}}}", "[".repeat(66), "]".repeat(66))),
        "nesting too deep"
    );
    assert!(keygen::read_flat_json(&format!("{{\"_a\": {}{}}}", "[".repeat(65), "]".repeat(65))).is_ok());
}

#[test]
fn d1_placeholder_named_like_prelude_variant_is_escaped() {
    // A parameter named None/Some/Ok/Err would be read by rustc as a variant pattern: it gets a `_` suffix.
    let result = run(&catalog(&[r#""k.v": "A {None} {Some} {Ok} {Err} b""#]));

    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let tr = keygen::render_tr(&result);
    assert!(tr.contains(
        "pub fn k_v(None_: impl Into<LocValue>, Some_: impl Into<LocValue>, Ok_: impl Into<LocValue>, \
         Err_: impl Into<LocValue>) -> String {"
    ));
    assert!(tr.contains("LocArg::new(\"None\", None_)"));
    assert_eq!(keygen::param_name("None"), "None_");
    assert_eq!(keygen::param_name("none"), "none");
}

#[test]
fn d2_bidi_control_in_english_text_does_not_reach_doc_comment() {
    // rustc denies U+202A..U+202E and U+2066..U+2069 in comments and literals; C# does not care.
    let result = run(&catalog(&[r#""k.bidi": "a\u202eb\u2066c\u2069d""#]));

    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let tr = keygen::render_tr(&result);
    assert!(tr.contains("/// English: \"a b c d\"\n"), "{tr}");
    assert!(
        !tr.chars()
            .any(|c| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
    );
}

#[test]
fn d3_invalid_escape_message_for_astral_char() {
    // C# reads one UTF-16 unit after the backslash: a lone surrogate, shown as U+FFFD.
    let error = keygen::read_flat_json("{\"a\": \"\\\u{1F600}\"}").expect_err("invalid escape");
    assert_eq!(error.message, "invalid escape '\\\u{FFFD}'");
    let bmp = keygen::read_flat_json("{\"a\": \"\\\u{65E5}\"}").expect_err("invalid escape");
    assert_eq!(bmp.message, "invalid escape '\\\u{65E5}'");
}

#[test]
fn m39_extra_catalog_order_is_utf16_ordinal() {
    // Mutation M39: paths are ordered by UTF-16 code units (C# string.CompareOrdinal), not by code point:
    // U+10000 (D800 DC00) sorts BEFORE U+E000 although its code point is larger.
    let astral = "C:\\\u{10000}\\en.json";
    let bmp = "C:\\\u{E000}\\en.json";
    for inputs in [
        [
            input(bmp, &catalog(&["\"second\": \"x\""])),
            input(astral, &catalog(&["\"first\": \"x\""])),
        ],
        [
            input(astral, &catalog(&["\"first\": \"x\""])),
            input(bmp, &catalog(&["\"second\": \"x\""])),
        ],
    ] {
        let result = plan(&inputs);
        assert_eq!(ids(&result), ["PRLOC007"]);
        assert!(result.diagnostics[0].message.contains(bmp));
        assert!(
            result.diagnostics[0]
                .message
                .contains(&format!("only '{astral}' (first by path)"))
        );
        assert!(keygen::render_tr(&result).contains("pub fn first("));
    }
}

#[test]
fn plural_group_params_follow_one_placeholder_order_when_one_adds_several() {
    // Mutation gap (verifier M14): `.one` introduces two placeholders that `.other` lacks; they join the
    // parameter chain in `.one` order (a, b), after the `.other` placeholders.
    let result = run(&catalog(&[r#""x.one": "{a} {b}""#, r#""x.other": "{c} many""#]));

    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let tr = keygen::render_tr(&result);
    assert!(
        tr.contains(
            "pub fn x(count: i64, c: impl Into<LocValue>, a: impl Into<LocValue>, b: impl Into<LocValue>) -> String {"
        ),
        "{tr}"
    );
    let bare = run(&catalog(&[r#""x.one": "{a} {b}""#, r#""x.other": "many""#]));
    assert!(
        keygen::render_tr(&bare)
            .contains("pub fn x(count: i64, a: impl Into<LocValue>, b: impl Into<LocValue>) -> String {")
    );
}
