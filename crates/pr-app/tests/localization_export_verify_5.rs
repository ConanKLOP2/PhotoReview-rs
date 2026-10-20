//! Adversarial verification of W8-04b: kill table for the lenient notes reader. The expected values
//! were confirmed against the .NET oracle (JsonDocument.Parse + ReadNotes semantics) with `vcmp_kill_vs_oracle`.
use pr_app::localization::export::read_notes;
use std::path::PathBuf;

type Case = (Vec<u8>, Vec<(&'static str, String)>);

fn nest_arrays(d: usize) -> Vec<u8> {
    let mut v = b"{\"a\":\"1\",\"n\":".to_vec();
    v.extend(std::iter::repeat_n(b'[', d));
    v.extend(std::iter::repeat_n(b']', d));
    v.push(b'}');
    v
}

fn nest_objects(d: usize) -> Vec<u8> {
    let mut v = b"{\"a\":\"1\",\"n\":".to_vec();
    for _ in 0..d {
        v.extend(b"{\"x\":");
    }
    v.extend(b"1");
    v.extend(std::iter::repeat_n(b'}', d));
    v.push(b'}');
    v
}

fn s(x: &str) -> Vec<u8> {
    x.as_bytes().to_vec()
}

fn bs(x: &str) -> Vec<u8> {
    x.replace('@', "\\").into_bytes()
}

fn ch(cp: u32) -> String {
    char::from_u32(cp).unwrap().to_string()
}

fn one(k: &'static str, v: &str) -> Vec<(&'static str, String)> {
    vec![(k, v.to_owned())]
}

fn cases() -> Vec<Case> {
    let mut c: Vec<Case> = Vec::new();
    let a1 = || one("a", "1");
    let none = || Vec::new();
    c.push((s(r#"{"a":"1","b":false}"#), a1()));
    c.push((s(r#"{"a":"1","b":true,"c":null}"#), a1()));
    c.push((s(r#"{"a":"1","b":fals}"#), none()));
    c.push((s(r#"{"a":"1","b":tru}"#), none()));
    c.push((s(r#"{"a":"1","b":nul}"#), none()));
    c.push((s(r#"{"a":"\q"}"#), none()));
    c.push((s(r#"{"a":"1","b":[1,2,[3,[4]],{"c":"x"}]}"#), a1()));
    c.push((s(r#"{"a":"1","b":[1,]}"#), a1()));
    c.push((s(r#"{"a":"1","b":[1 2]}"#), none()));
    c.push((s(r#"{"a":"1","b":[,]}"#), none()));
    c.push((s(r#"{"a":"1","b":[1,,2]}"#), none()));
    c.push((s(r#"{"a":"1","b":[1}"#), none()));
    c.push((s(r#"{"a":"1","b":[}"#), none()));
    c.push((s(r#"{"a":"1","b":[1,2"#), none()));
    c.push((s("{\"a\":\"x\u{1}y\"}"), none()));
    c.push((s("{\"a\":\"x\ty\"}"), none()));
    c.push((s("{\"a\":\"x\u{1f}y\"}"), none()));
    c.push((s("{\"a\":\"x\u{20}y\"}"), one("a", "x y")));
    c.push((s(r#"{"a":"1",}"#), a1()));
    c.push((s(r#"{,}"#), none()));
    c.push((s(r#"{"a":"1",,}"#), none()));
    c.push((s(r#"{"a":"1""b":"2"}"#), none()));
    c.push((s(r#"{"a" "1"}"#), none()));
    c.push((s(r#"{"a":"1","b":"2"}"#), vec![("a", "1".into()), ("b", "2".into())]));
    c.push((s(r#"{"a":"1","a":"2"}"#), one("a", "2")));
    c.push((s(r#"{"_a":"1","b":3,"c":"x"}"#), one("c", "x")));
    // comments and whitespace
    c.push((
        s("{\"a\":\"1\"/*c*/,\"b\":\"2\"}"),
        vec![("a", "1".into()), ("b", "2".into())],
    ));
    c.push((s("{\"a\"://c\n\"1\"}"), a1()));
    c.push((s("{\"a\":\"1\"//c\n}"), a1()));
    c.push((s("{\"a\":\"1\"//c\r\n}"), a1()));
    c.push((s("{\"a\":\"1\"//c\r}"), a1()));
    c.push((s("/**/{\"a\":\"1\"}"), a1()));
    c.push((s("/***/{\"a\":\"1\"}"), a1()));
    c.push((s("/* a * b / c */{\"a\":\"1\"}"), a1()));
    c.push((s("//\n{\"a\":\"1\"}"), a1()));
    c.push((s("//x\n//y\n{\"a\":\"1\"}"), a1()));
    c.push((s("/*x*//*y*/{\"a\":\"1\"}"), a1()));
    c.push((s("{\"a\":\"1\"}//c"), a1()));
    c.push((s("{\"a\":\"1\"}/*c*/"), a1()));
    c.push((s("{\"a\":\"1\"} /"), none()));
    c.push((s("/x{\"a\":\"1\"}"), none()));
    c.push((s("{\"a\":\"1\"}/* "), none()));
    c.push((s("/*/ {\"a\":\"1\"}"), none()));
    c.push((s("/*/ */{\"a\":\"1\"}"), a1()));
    c.push((s("{\"a\":\"1\"} x"), none()));
    c.push((s("{\"a\":\"1\"}{"), none()));
    c.push((s("{/*c*/\"a\":\"1\"}"), a1()));
    c.push((s("{\"a\":/*c*/\"1\"}"), a1()));
    c.push((s("{\"a\":\"1\",/*c*/}"), a1()));
    c.push((s("{\"a\":\"1\",\"b\":[1,/*c*/2//c\n]}"), a1()));
    c.push((s("{\"a\":\"1\",\"b\":[/*c*/]}"), a1()));
    c.push((s("{\"a\":\"1\",\"b\":[1/*c*/]}"), a1()));
    c.push((s("{\"a\":\"1\",\"b\":[1/*c*/,/*d*/2]}"), a1()));
    c.push((s("\u{feff}{\"a\":\"1\"}"), a1()));
    c.push((s("{\"a\":\"1\"}\u{feff}"), none()));
    c.push((s(" \t\r\n{\"a\":\"1\"} \t\r\n"), a1()));
    c.push((s("\u{b}{\"a\":\"1\"}"), none()));
    c.push((s("[]"), none()));
    c.push((s("\"a\""), none()));
    c.push((s(""), none()));
    c.push((s("{"), none()));
    c.push((s("{\"a\""), none()));
    c.push((s("{\"a\":"), none()));
    c.push((s("{\"a\":\"1\""), none()));
    c.push((s("{\"a\":\"1\",\"b\":-}"), none()));
    c.push((s("{\"a\":\"1\",\"b\":-0}"), a1()));
    c.push((s("{\"a\":\"1\",\"b\":1.5e+3}"), a1()));
    c.push((s("{\"a\":\"1\",\"b\":01}"), none()));
    c.push((s("{\"a\":\"1\",\"b\":1.}"), none()));
    c.push((s("{\"a\":\"1\",\"b\":1e}"), none()));
    c.push((s("{\"a\":\"1\",\"b\":.5}"), none()));
    c.push((s("{\"a\":\"1\",\"b\":+1}"), none()));
    c.push((s(r#"{"a":"\ud83d"}"#), none()));
    c.push((s(r#"{"a":"\ude00"}"#), none()));
    c.push((s(r#"{"a":"\u12"}"#), none()));
    c.push((s(r#"{"a":"\u12g4"}"#), none()));
    // '@' stands for a backslash in bs(); ch() builds an expected scalar from its code point
    c.push((
        bs("{\"a\":\"@@@/@b@f@n@r@t@\"@u0041\"}"),
        one("a", &format!("@/{}{}\n\r\t\"A", '\u{8}', '\u{c}').replace('@', "\\")),
    ));
    c.push((bs("{\"a\":\"@ud83d@ude00\"}"), one("a", &ch(0x1F600))));
    c.push((bs("{\"a\":\"@uDBFF@uDFFF\"}"), one("a", &ch(0x10FFFF))));
    c.push((bs("{\"a\":\"@uD800@uDC00\"}"), one("a", &ch(0x10000))));
    c.push((bs("{\"a\":\"@uD801@uDC37\"}"), one("a", &ch(0x10437))));
    c.push((bs("{\"a\":\"@ud83d@u0041\"}"), none()));
    c.push((bs("{\"a\":\"@ud83dx\"}"), none()));
    c.push((bs("{\"a\":\"@ud83d@\"}"), none()));
    c.push((bs("{\"a\":\"@ud83d@xdc00\"}"), none()));
    c.push((bs("{\"a\":\"@ud83d@@udc00\"}"), none()));
    c.push((
        bs("{\"a\":\"@u00E9@u00e9@u0041@uabcd@uABCD@u0030@u0039\"}"),
        one("a", &format!("{}{}A{}{}09", ch(0xe9), ch(0xe9), ch(0xabcd), ch(0xabcd))),
    ));
    c.push((bs("{\"a\":\"@u00g0\"}"), none()));
    c.push((bs("{\"a\":\"@u00G0\"}"), none()));
    c.push((bs("{\"a\":\"@u 123\"}"), none()));
    c.push((bs("{\"a\":\"@u/000\"}"), none()));
    c.push((bs("{\"a\":\"@u:000\"}"), none()));
    c.push((bs("{\"a\":\"@u@000\"}"), none()));
    c.push((bs("{\"a\":\"@u`000\"}"), none()));
    c.push((bs("{\"a\":\"@uF000\"}"), one("a", &ch(0xF000))));
    // depth: object root counts as 1, so 63 nested containers inside are accepted, 64 are not
    for d in [1usize, 62, 63, 64, 65] {
        let ok = d <= 63;
        c.push((nest_arrays(d), if ok { a1() } else { none() }));
        c.push((nest_objects(d), if ok { a1() } else { none() }));
    }
    // many siblings: depth must go back down after each container
    let mut v = b"{\"a\":\"1\",\"n\":[".to_vec();
    for i in 0..100 {
        if i > 0 {
            v.push(b',');
        }
        v.extend(b"[]");
    }
    v.extend(b"]}");
    c.push((v, a1()));
    let mut v = b"{\"a\":\"1\"".to_vec();
    for i in 0..100 {
        v.extend(format!(",\"o{i}\":{{}}").as_bytes());
    }
    v.push(b'}');
    c.push((v, a1()));
    let mut v = b"{\"a\":\"1\",\"n\":[".to_vec();
    for i in 0..100 {
        if i > 0 {
            v.push(b',');
        }
        v.extend(b"{\"k\":[1]}");
    }
    v.extend(b"]}");
    c.push((v, a1()));
    c
}

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02X}")).collect()
}

fn run(bytes: &[u8], tag: &str) -> Vec<(String, String)> {
    let d = std::env::temp_dir().join(format!("verify-w8-04b-e-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("en.notes.json");
    std::fs::write(&p, bytes).unwrap();
    let r = read_notes(&p).into_iter().collect();
    let _ = std::fs::remove_dir_all(&d);
    r
}

#[test]
fn kill_notes_table() {
    let mut bad = Vec::new();
    for (i, (bytes, expected)) in cases().iter().enumerate() {
        let mut want: Vec<(String, String)> = expected.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
        want.sort();
        let got = run(bytes, "t");
        if got != want {
            bad.push(format!(
                "case {i}: {:?} got {got:?} want {want:?}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(60)])
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn vcmp_gen_kill_cases() {
    let Ok(dir) = std::env::var("NOTES_DIR") else { return };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (i, (bytes, _)) in cases().iter().enumerate() {
        std::fs::write(dir.join(format!("{i}.bin")), bytes).unwrap();
    }
}

#[test]
fn vcmp_kill_vs_oracle() {
    let Ok(dir) = std::env::var("NOTES_DIR") else { return };
    let dir = PathBuf::from(dir);
    let mut bad = Vec::new();
    for (i, (_, expected)) in cases().iter().enumerate() {
        let mut want: Vec<(String, String)> = expected.iter().map(|(k, v)| (hex(k), hex(v))).collect();
        want.sort();
        let cs = std::fs::read_to_string(dir.join(format!("{i}.out"))).unwrap();
        let got: Vec<(String, String)> = cs
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let (k, v) = l.split_once(' ').unwrap_or((l, ""));
                (k.to_owned(), v.to_owned())
            })
            .collect();
        if got != want {
            bad.push(format!("case {i}: oracle {got:?} expected {want:?}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
