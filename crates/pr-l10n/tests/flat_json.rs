//! Port of `FlatJsonReaderDifferentialTests` (C# baseline d92d6aae): the compile-time reader (`keygen::read_flat_json`)
//! and the runtime parser (`LanguageCatalog::try_parse`) must agree on which catalogs are valid and on what they
//! contain. In C# the reference is `System.Text.Json`; here it is the hand-written runtime parser, whose own
//! behaviour is pinned against C# by the `l10n-catalog` and `l10n-flat-json` goldens.
//! Each test names its C# twin in a comment.

#[path = "../build/keygen.rs"]
#[allow(dead_code)] // the whole generator module is compiled here; the tests use part of it
mod keygen;

use pr_l10n::catalog::LanguageCatalog;

/// What the runtime says about a file.
#[derive(Debug, PartialEq, Eq)]
enum Runtime {
    /// Not valid JSON, not an object, a non-string entry value, or unreadable text.
    Reject,
    /// A usable flat catalog; entries when the runtime also accepted `_meta.code`.
    Accept(Option<Vec<(String, String)>>),
    /// The runtime read a lone surrogate inside `_meta` (it reads every `_meta` name and the known string values; the
    /// C# reference never materialises them). Not comparable with the reader.
    UnreadableMeta,
}

fn runtime(text: &str) -> Runtime {
    // The runtime reads the file through a reader that strips a BOM (the loader does the same).
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut warnings = Vec::new();
    let parsed = LanguageCatalog::try_parse(text, "t", &mut warnings);
    let any = |suffix: &str| warnings.iter().any(|w| w.contains(suffix));
    if any(": invalid JSON (") || any(": root must be a JSON object") || any("is not a string, ignored") {
        return Runtime::Reject;
    }
    if any(": unreadable text (") {
        return if text.contains("_meta") {
            Runtime::UnreadableMeta
        } else {
            Runtime::Reject
        };
    }
    Runtime::Accept(parsed.map(|c| c.entries().to_vec()))
}

fn generator(text: &str) -> Option<Vec<(String, String)>> {
    keygen::read_flat_json(text)
        .ok()
        .map(|entries| entries.into_iter().map(|e| (e.key, e.value)).collect())
}

/// Last value wins at the first position (what the runtime catalog does with duplicate names).
fn dedup(entries: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (k, v) in entries {
        match out.iter_mut().find(|(ok, _)| *ok == k) {
            Some(slot) => slot.1 = v,
            None => out.push((k, v)),
        }
    }
    out
}

const TOKENS: &[&str] = &[
    "{",
    "}",
    "[",
    "]",
    ",",
    ":",
    "\"",
    "\"a\"",
    "\"b\"",
    "\"_m\"",
    "\"key.one\"",
    "\"v\"",
    "\"\\n\"",
    "\"\\u0041\"",
    "\"\\ud83d\\ude00\"",
    "\"\\ud800\"",
    "\"\\udc00x\"",
    "\"\\uD83D\"",
    "\"\\u00\"",
    "\"\\x\"",
    "\"tab\there\"",
    "1",
    "01",
    "-",
    "-0",
    "1.",
    "1.5e+3",
    "1e",
    "true",
    "tru",
    "null",
    "nul",
    " ",
    "\n",
    "\t",
    "// c\n",
    "/* c */",
    "/*",
    "//",
    "\u{FEFF}",
    "\u{A0}",
    "\u{2028}",
    "\0",
    "'",
    "a",
    "日本",
    "\u{1F600}",
    "\r",
];

const VALID: &str = "{\n  // comment\n  \"_meta\": { \"code\": \"en\", \"n\": [1, 2.5e3, -0, true, null, {\"x\": []}] },\n  \"a.b\": \"Hello {name}\", /* block */\n  \"c\": \"\\u00e9\\ud83d\\ude00\\n\\\"q\\\"\",\n}";

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

fn soup(r: &mut XorShift) -> String {
    let mut s = String::new();
    if r.below(4) != 0 {
        s.push('{');
    }
    for _ in 0..r.below(14) {
        s.push_str(TOKENS[r.below(TOKENS.len())]);
    }
    if r.below(3) != 0 {
        s.push('}');
    }
    s
}

fn mutate_valid(r: &mut XorShift) -> String {
    let chars: Vec<char> = VALID.chars().collect();
    let mut working = chars.clone();
    for _ in 0..(1 + r.below(3)) {
        let i = r.below(working.len());
        match r.below(3) {
            0 => {
                working.remove(i);
            }
            1 => {
                let token: Vec<char> = TOKENS[r.below(TOKENS.len())].chars().collect();
                for (offset, c) in token.into_iter().enumerate() {
                    working.insert(i + offset, c);
                }
            }
            _ => working[i] = chars[r.below(chars.len())],
        }
        if working.is_empty() {
            working = chars.clone();
        }
    }
    working.into_iter().collect()
}

fn escape(text: &str) -> String {
    text.chars()
        .map(|c| {
            if !(' '..='~').contains(&c) {
                format!("\\u{:04x}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}

#[test]
fn valid_both_accept() {
    // C#: FlatJsonReaderDifferentialTests.Valid_BothAccept
    let from_reader = generator(VALID).expect("reader accepts the valid sample");

    match runtime(VALID) {
        Runtime::Accept(Some(entries)) => assert_eq!(entries, from_reader),
        other => panic!("runtime verdict {other:?}"),
    }
    assert_eq!(
        from_reader,
        [
            ("a.b".to_owned(), "Hello {name}".to_owned()),
            ("c".to_owned(), "é\u{1F600}\n\"q\"".to_owned())
        ]
    );
}

#[test]
fn soup_agrees() {
    // C#: FlatJsonReaderDifferentialTests.Soup_Agrees (the seeds are our own; the golden `l10n-flat-json` carries
    // the C# verdicts for the C# soup)
    for seed in 1..=4 {
        let mut r = XorShift::new(seed);
        let mut mismatches: Vec<String> = Vec::new();
        for i in 0..20_000 {
            let text = if i % 2 == 0 { soup(&mut r) } else { mutate_valid(&mut r) };
            let actual = generator(&text);
            match runtime(&text) {
                Runtime::UnreadableMeta => {}
                Runtime::Reject => {
                    if actual.is_some() {
                        mismatches.push(format!("runtime=reject generator=accept :: {}", escape(&text)));
                    }
                }
                Runtime::Accept(entries) => match (&actual, entries) {
                    (None, _) => mismatches.push(format!("runtime=accept generator=reject :: {}", escape(&text))),
                    (Some(a), Some(e)) if dedup(a.clone()) != e => {
                        mismatches.push(format!("entries differ :: {}", escape(&text)));
                    }
                    _ => {}
                },
            }
            if mismatches.len() >= 10 {
                break;
            }
        }
        assert!(mismatches.is_empty(), "seed {seed}: {}", mismatches.join(" || "));
    }
}

#[test]
fn lone_surrogate_in_skipped_meta_agrees_with_runtime() {
    // C#: FlatJsonReaderDifferentialTests.LoneSurrogateInSkippedMeta_AgreesWithRuntime (R33). The reader must accept
    // these (it never materialises skipped values); the Rust runtime parser reads `_meta` names (contract §2), so it
    // reports `unreadable text` only for the one text with a lone surrogate NAME inside `_meta`.
    for text in [
        r#"{ "_meta": { "n": "\ud800" }, "a": "x" }"#,
        r#"{ "_meta": [ "\udc00x" ], "a": "x" }"#,
        r#"{ "_meta": { "\ud83d": 1 }, "a": "x" }"#,
        r#"{ "_meta": "\ud800", "a": "x" }"#,
        r#"{ "_meta": { "n": "\ud83d\ude00" }, "a": "x" }"#,
    ] {
        let actual = generator(text);

        assert_eq!(actual, Some(vec![("a".to_owned(), "x".to_owned())]), "{text}");
        assert!(
            matches!(runtime(text), Runtime::Accept(_) | Runtime::UnreadableMeta),
            "{text}"
        );
    }
}

#[test]
fn deep_and_huge() {
    // C#: FlatJsonReaderDifferentialTests.Deep_And_Huge
    let deep = format!("{{\"_x\":{}{}}}", "[".repeat(5000), "]".repeat(5000));
    assert!(generator(&deep).is_none());
    assert_eq!(runtime(&deep), Runtime::Reject);

    let big = format!(
        "{{{}}}",
        (0..100_000)
            .map(|i| format!("\"k{i}\":\"v\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(generator(&big).expect("huge file").len(), 100_000);
}

#[test]
fn nesting_depth_asymmetry_is_pinned() {
    // Contract §28: the reader counts depth from the value of the key (65 nested arrays pass), the runtime parser
    // counts the root object too (64 nested arrays are already invalid). The goldens pin both.
    let arrays = |n: usize| format!("{{\"_x\":{}{}}}", "[".repeat(n), "]".repeat(n));

    assert!(generator(&arrays(65)).is_some());
    assert_eq!(runtime(&arrays(63)), Runtime::Accept(None));
    assert_eq!(runtime(&arrays(64)), Runtime::Reject);
}
