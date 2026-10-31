//! Killers for survivors of the cl-mut-misc rerun (updates/checker.rs `Scanner::value`).
use pr_core::updates::checker::UpdateChecker;
use pr_core::updates::version::{AppVersion, UpdateCheckStatus, UpdateFailure};

fn current() -> AppVersion {
    AppVersion {
        major: 1,
        minor: 0,
        patch: 0,
        is_prerelease: false,
    }
}

/// Document whose deepest container sits at nesting level `levels` (the root object is level 1).
fn nested_objects(levels: usize) -> String {
    let inner = levels - 1;
    format!(
        r#"{{"tag_name":"v2.0.0","x":{}1{}}}"#,
        r#"{"a":"#.repeat(inner),
        "}".repeat(inner)
    )
}

#[test]
fn nested_object_depth_limit_is_64_inclusive() {
    // C#: JsonDocument.Parse default MaxDepth 64. Mutants checker.rs:768:26 (+ to - / *), 768:30 (> to >=),
    // 772:35 (depth + 1 to depth - 1 / depth * 1) in Scanner::value.
    let ok = interpret_within_deadline(&nested_objects(64));
    assert_eq!(ok.status(), UpdateCheckStatus::UpdateAvailable);
    let too_deep = interpret_within_deadline(&nested_objects(65));
    assert_eq!(too_deep.failure(), UpdateFailure::BadResponse);
    let far_too_deep = interpret_within_deadline(&nested_objects(200));
    assert_eq!(far_too_deep.failure(), UpdateFailure::BadResponse);
}

/// Runs `Interpret` on a worker thread and fails fast (instead of hanging the whole binary) when the scanner does not advance.
fn interpret_within_deadline(body: &str) -> pr_core::updates::version::UpdateCheckResult {
    let (tx, rx) = std::sync::mpsc::channel();
    let owned = body.to_owned();
    std::thread::spawn(move || {
        let _ = tx.send(UpdateChecker::interpret(&owned, current()));
    });
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("scanner did not terminate for {body:?}"))
}

#[test]
fn scanner_cursor_always_advances_and_terminates() {
    // Former timeouts (detected only by the 300 s cargo-mutants timeout): checker.rs:647:22 skip_ws `+=` to `*=`,
    // 691:30 and 693:30 string escape `+=` to `-=`, 719:30 scalar `+=` to `*=`, 729:22 digits `+=` to `*=`.
    let docs = [
        (r#" { "tag_name" : "v2.0.0" } "#, "whitespace"),
        (r#"{"tag_name":"v2.0.0","u":"a\nb"}"#, "short escape"),
        (
            concat!(r#"{"tag_name":"v2.0.0","u":"a"#, r"\", r#"u0041b"}"#),
            "unicode escape",
        ),
        (
            "{\"tag_name\":\"v2.0.0\",\"u\":\"caf\u{e9} \u{1f600}\"}",
            "non-ASCII scalar",
        ),
        (r#"{"tag_name":"v2.0.0","n":12345}"#, "digits"),
    ];
    for (body, what) in docs {
        let result = interpret_within_deadline(body);
        assert_eq!(result.status(), UpdateCheckStatus::UpdateAvailable, "{what}");
    }
}

#[test]
fn escaped_property_name_is_decoded_by_the_scanner() {
    // Mutant checker.rs:692:30 (`self.pos += 1` after the backslash, `+=` to `*=`): the escape letter was then read from the
    // backslash itself, so an escaped name decoded to a different string. C#: JsonDocument decodes property names.
    let body = concat!(r#"{"tag"#, r"\", r#"u005fname":"v2.0.0"}"#);
    let result = interpret_within_deadline(body);
    assert_eq!(result.status(), UpdateCheckStatus::UpdateAvailable);
    assert_eq!(result.latest_version(), Some("2.0.0"));
}
