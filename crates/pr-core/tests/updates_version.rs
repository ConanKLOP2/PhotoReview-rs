//! Port of the C# `Updates` tests (W1-10): AppVersion, UpdateUrlPolicy, UpdateCheckResult.

use std::cmp::Ordering;

use pr_core::updates::version::{
    AppVersion, RELEASES_PAGE_URL, UpdateCheckResult, UpdateCheckStatus, UpdateFailure, validate_update_url,
};
use pr_test_support::golden;

fn v(text: &str) -> AppVersion {
    AppVersion::try_parse(Some(text)).unwrap_or_else(|| panic!("should parse: {text:?}"))
}

fn sign(o: Ordering) -> i32 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

// C#: AppVersionTests.CompareTo_OrdersVersionsNumerically
#[test]
fn compare_to_orders_versions_numerically() {
    let rows = [
        ("2.0.93", "2.0.93", 0),
        ("2.0.94", "2.0.93", 1),
        ("2.0.92", "2.0.93", -1),
        ("2.1.0", "2.0.99", 1),
        ("3.0.0", "2.99.99", 1),
        ("2.0.100", "2.0.99", 1),
        ("v2.0.94", "2.0.93", 1),
        ("V2.0.93", "2.0.93", 0),
        ("2.0.93", "2.0.93+1a2b3c4d", 0),
        ("2.0.94", "2.0.93+1a2b3c4d.dirty", 1),
        ("2.0.93", "2.0.93-beta.1", 1),
        ("2.0.93-beta.1", "2.0.93", -1),
        ("2.0.94-rc.1", "2.0.93", 1),
    ];
    for (l, r, expected) in rows {
        assert_eq!(sign(v(l).cmp(&v(r))), expected, "{l} vs {r}");
    }
}

// C#: AppVersionTests.TryParse_Garbage_ReturnsFalse
#[test]
fn try_parse_garbage_returns_none() {
    assert_eq!(AppVersion::try_parse(None), None);
    for text in [
        "",
        "   ",
        "garbage",
        "2.0",
        "2.0.93.1",
        "2.x.93",
        "2..93",
        "-1.0.0",
        "2.0.93-",
        "99999999999.0.0",
        "v",
    ] {
        assert_eq!(AppVersion::try_parse(Some(text)), None, "{text:?}");
    }
}

// C#: AppVersionTests.TryParse_Prerelease_IsFlagged
#[test]
fn try_parse_prerelease_is_flagged() {
    let p = v("v2.1.0-beta.2+abc");
    assert!(p.is_prerelease);
    assert_eq!(p.to_string(), "2.1.0");
    assert!(!v("2.1.0").is_prerelease);
}

// C#: AppVersionOperatorTests.Operators_MatchOrdering
#[test]
fn operators_match_ordering() {
    let rows = [
        ("2.0.92", "2.0.93", -1),
        ("2.0.93", "2.0.93", 0),
        ("2.0.94", "2.0.93", 1),
        ("2.0.93-beta.1", "2.0.93", -1),
    ];
    for (left, right, s) in rows {
        let (l, r) = (v(left), v(right));
        assert_eq!(l < r, s < 0, "{left} < {right}");
        assert_eq!(l > r, s > 0, "{left} > {right}");
        assert_eq!(l <= r, s <= 0, "{left} <= {right}");
        assert_eq!(l >= r, s >= 0, "{left} >= {right}");
    }
}

// C#: AppVersionOperatorTests.TryParse_RejectsNonDigits
#[test]
fn try_parse_rejects_non_digits() {
    for text in ["1.2.3\0", "1.2\0.3", "1.2.3x", "1.2.-3", "+1.2.3", "-1.2.3"] {
        assert_eq!(AppVersion::try_parse(Some(text)), None, "{text:?}");
    }
}

// C#: AppVersionOperatorTests.TryParse_RejectsEmptyComponents
#[test]
fn try_parse_rejects_empty_components() {
    for text in ["1..3", ".2.3", "1.2."] {
        assert_eq!(AppVersion::try_parse(Some(text)), None, "{text:?}");
    }
}

// C#: UpdateUrlPolicyTests.Validate_ProjectUrl_ReturnsUrl
#[test]
fn validate_project_url_returns_url() {
    for url in [
        "https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v2.0.94",
        "https://github.com/ConanKLOP2/PhotoReview-rs/releases",
        "https://github.com/ConanKLOP2/PhotoReview-rs",
        "https://GITHUB.com/conanklop2/photoreview-rs/releases/latest",
    ] {
        assert!(validate_update_url(Some(url)).is_some(), "{url}");
    }
    assert_eq!(
        validate_update_url(Some(RELEASES_PAGE_URL)).as_deref(),
        Some(RELEASES_PAGE_URL)
    );
}

// C#: UpdateUrlPolicyTests.Validate_Other_ReturnsNull
#[test]
fn validate_other_returns_none() {
    assert_eq!(validate_update_url(None), None);
    for url in [
        "",
        "not a url",
        "http://github.com/ConanKLOP2/PhotoReview-rs/releases",
        "https://github.com/Evil/PhotoReview/releases",
        "https://github.com/ConanKLOP2/PhotoReview-rsEvil/releases",
        "https://github.com/ConanKLOP2/Other",
        "https://github.com.evil.example/ConanKLOP2/PhotoReview-rs/releases",
        "https://evil.example/https://github.com/ConanKLOP2/PhotoReview-rs",
        "https://github.com@evil.example/ConanKLOP2/PhotoReview-rs/releases",
        "https://user:pw@github.com/ConanKLOP2/PhotoReview-rs/releases",
        "https://github.com:8443/ConanKLOP2/PhotoReview-rs/releases",
        "https://github.com/ConanKLOP2/PhotoReview-rs/../Evil/x",
        "file:///C:/Windows/System32/calc.exe",
        "ms-settings:",
        "javascript:alert(1)",
    ] {
        assert_eq!(validate_update_url(Some(url)), None, "{url}");
    }
}

// C#: (no direct test) UpdateCheckResult.UpToDate
#[test]
fn update_check_result_up_to_date_sets_version_only() {
    let r = UpdateCheckResult::up_to_date("2.0.93");
    assert_eq!(r.status(), UpdateCheckStatus::UpToDate);
    assert_eq!(r.latest_version(), Some("2.0.93"));
    assert_eq!(r.download_url(), None);
    assert_eq!(r.failure(), UpdateFailure::None);
}

// C#: (no direct test) UpdateCheckResult.UpdateAvailable
#[test]
fn update_check_result_update_available_sets_version_and_url() {
    let r = UpdateCheckResult::update_available("2.0.94", "https://example.invalid/x");
    assert_eq!(r.status(), UpdateCheckStatus::UpdateAvailable);
    assert_eq!(r.latest_version(), Some("2.0.94"));
    assert_eq!(r.download_url(), Some("https://example.invalid/x"));
    assert_eq!(r.failure(), UpdateFailure::None);
}

// C#: (no direct test) UpdateCheckResult.Failed
#[test]
fn update_check_result_failed_sets_failure_only() {
    let r = UpdateCheckResult::failed(UpdateFailure::RateLimited);
    assert_eq!(r.status(), UpdateCheckStatus::Failed);
    assert_eq!(r.latest_version(), None);
    assert_eq!(r.download_url(), None);
    assert_eq!(r.failure(), UpdateFailure::RateLimited);
}

// C#: golden app-version
#[test]
fn golden_app_version() {
    let g = golden::load("app-version");
    for case in &g.cases {
        match golden::str_of(case, "kind") {
            "parse" => {
                let text = case["text"].as_str();
                let parsed = AppVersion::try_parse(text);
                if golden::bool_of(case, "ok") {
                    let p = parsed.unwrap_or_else(|| panic!("should parse: {case}"));
                    assert_eq!(i64::from(p.major), golden::i64_of(case, "major"), "{case}");
                    assert_eq!(i64::from(p.minor), golden::i64_of(case, "minor"), "{case}");
                    assert_eq!(i64::from(p.patch), golden::i64_of(case, "patch"), "{case}");
                    assert_eq!(p.is_prerelease, golden::bool_of(case, "prerelease"), "{case}");
                    assert_eq!(p.to_string(), golden::str_of(case, "to_string"), "{case}");
                } else {
                    assert_eq!(parsed, None, "{case}");
                }
            }
            "compare" => {
                let (a, b) = (v(golden::str_of(case, "a")), v(golden::str_of(case, "b")));
                assert_eq!(i64::from(sign(a.cmp(&b))), golden::i64_of(case, "sign"), "{case}");
                assert_eq!(a < b, golden::bool_of(case, "lt"), "{case}");
                assert_eq!(a > b, golden::bool_of(case, "gt"), "{case}");
                assert_eq!(a <= b, golden::bool_of(case, "le"), "{case}");
                assert_eq!(a >= b, golden::bool_of(case, "ge"), "{case}");
                assert_eq!(a == b, golden::bool_of(case, "equal"), "{case}");
            }
            other => panic!("unknown kind {other}"),
        }
    }
}

/// DIVERGENCE (DV-22, P-60): the goldens come from the C# oracle, which follows `ConanKLOP2/PhotoReview`; this build
/// follows `ConanKLOP2/PhotoReview-rs`. Rewrites the C# project path (ASCII case-insensitive) in a golden input or
/// expected value. Everything else in a case stays byte-for-byte the oracle's.
fn divergence_adjusted_repo(s: &str) -> String {
    const CS: &str = "ConanKLOP2/PhotoReview";
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < s.len() {
        match s.as_bytes().get(i..i + CS.len()) {
            Some(w) if w.eq_ignore_ascii_case(CS.as_bytes()) => {
                out.push_str(&s[i..i + CS.len()]);
                out.push_str("-rs");
                i += CS.len();
            }
            _ => {
                let c = s[i..].chars().next().expect("char");
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

// C#: golden update-url-policy (with the controlled DV-22 repo adjustment)
#[test]
fn golden_update_url_policy() {
    let g = golden::load("update-url-policy");
    for case in &g.cases {
        let (url, expected) = match case["url"].as_str() {
            // Cases where the C# path is not one contiguous `ConanKLOP2/PhotoReview` (dot segments, percent-encoding):
            // the repository segment is adjusted by hand.
            Some("https://github.com/ConanKLOP2/./PhotoReview") => (
                Some("https://github.com/ConanKLOP2/./PhotoReview-rs".to_owned()),
                Some("https://github.com/ConanKLOP2/PhotoReview-rs".to_owned()),
            ),
            Some("https://github.com/ConanKLOP2/x/../PhotoReview") => (
                Some("https://github.com/ConanKLOP2/x/../PhotoReview-rs".to_owned()),
                Some("https://github.com/ConanKLOP2/PhotoReview-rs".to_owned()),
            ),
            Some("https://github.com/%43onanKLOP2/PhotoReview/x") => (
                Some("https://github.com/%43onanKLOP2/PhotoReview-rs/x".to_owned()),
                Some("https://github.com/ConanKLOP2/PhotoReview-rs/x".to_owned()),
            ),
            Some("https://github.com/conanklop2/%50hotoreview") => (
                Some("https://github.com/conanklop2/%50hotoreview-rs".to_owned()),
                Some("https://github.com/conanklop2/Photoreview-rs".to_owned()),
            ),
            url => (
                url.map(divergence_adjusted_repo),
                case["result"].as_str().map(divergence_adjusted_repo),
            ),
        };
        assert_eq!(validate_update_url(url.as_deref()), expected, "{case}");
    }
}

// DV-22: the C# project's repository is no longer an allowed update target; this build's own repository is.
#[test]
fn validate_rejects_the_csharp_repo_and_accepts_the_rust_repo() {
    for url in [
        "https://github.com/ConanKLOP2/PhotoReview",
        "https://github.com/ConanKLOP2/PhotoReview/releases",
        "https://github.com/ConanKLOP2/PhotoReview/releases/tag/v2.0.94",
        "https://github.com/conanklop2/photoreview/releases",
        "https://github.com/OtherOwner/PhotoReview-rs/releases",
        "https://github.com/ConanKLOP2/PhotoReview-rsX/releases",
        "https://github.com/ConanKLOP2/PhotoReview-r",
    ] {
        assert_eq!(validate_update_url(Some(url)), None, "{url}");
    }
    assert_eq!(
        validate_update_url(Some("https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v1.0.0")).as_deref(),
        Some("https://github.com/ConanKLOP2/PhotoReview-rs/releases/tag/v1.0.0")
    );
    assert_eq!(
        validate_update_url(Some("https://GitHub.com/conanklop2/photoreview-RS")).as_deref(),
        Some("https://github.com/conanklop2/photoreview-RS")
    );
    // The derived constants all point at the one configured repository, and are themselves accepted.
    assert_eq!(pr_core::updates::UPDATE_REPO, "ConanKLOP2/PhotoReview-rs");
    assert_eq!(
        RELEASES_PAGE_URL,
        format!("https://github.com/{}/releases", pr_core::updates::UPDATE_REPO)
    );
    assert_eq!(
        validate_update_url(Some(RELEASES_PAGE_URL)).as_deref(),
        Some(RELEASES_PAGE_URL)
    );
}

// C#: (no direct test) UpdateUrlPolicy.Validate, System.Uri escape edge cases. Every expected value below was
// produced by the C# UpdateUrlPolicy.Validate (e441d67a) on .NET 10.
#[test]
fn validate_matches_net_uri_escape_edge_cases() {
    const P: &str = "https://github.com/ConanKLOP2/PhotoReview-rs";
    let rows: &[(&str, &str)] = &[
        // A lead byte followed by an invalid continuation stays verbatim (case kept).
        ("/%c3%c3", "/%c3%c3"),
        ("/%c3%28", "/%c3%28"),
        ("/%e0%41", "/%e0A"),
        // An invalid `%` swallows the next two chars unexamined, so an escape right after it is not decoded.
        ("/%%2E", "/%25%2E"),
        ("/%%41", "/%25%41"),
        ("/%A%2E", "/%25A%2E"),
        ("/%zz%2E", "/%25zz."),
        ("/%%%2E", "/%25%25%2E"),
        ("/%%%%2E", "/%25%25%25."),
        ("?%A%2E", "?%25A%2E"),
        ("#%%2E", "#%25%2E"),
        ("/x/%%2E%2E/y", "/x/%25%2E./y"),
        ("/x/%2E%2E/y", "/y"),
        // ...and it works on UTF-16 units: the emoji's surrogate pair is split, each half becomes U+FFFD.
        ("/%%\u{1F600}", "/%25%25%EF%BF%BD%EF%BF%BD"),
        // Decoded UTF-8 must be an IRI char (RFC 3987 ucschar; iprivate only in the query), else kept verbatim.
        ("/%c2%85", "/%c2%85"),
        ("/%c2%a0", "/%C2%A0"),
        ("/%ef%bf%be", "/%ef%bf%be"),
        ("/%ee%80%80", "/%ee%80%80"),
        ("?%ee%80%80", "?%EE%80%80"),
        ("#%ee%80%80", "#%ee%80%80"),
        ("/%f3%b0%80%80", "/%f3%b0%80%80"),
        ("?%f3%b0%80%80", "?%F3%B0%80%80"),
        ("/%f3%a0%80%81", "/%f3%a0%80%81"),
        ("/%f3%a1%80%80", "/%F3%A1%80%80"),
        ("/%e2%80%8e", "/%E2%80%8E"),
        // On the IRI rewrite path the fragment loses its trailing white space (also a decoded `%20`).
        ("#\u{e9}%20", "#%C3%A9"),
        ("#\u{e9}%2e%20", "#%C3%A9."),
        ("#%41%20", "#A"),
        ("#\u{e9}%2e%20%20", "#%C3%A9."),
        ("#\u{e9}\t%20", "#%C3%A9"),
        // Literal CR, tab and LF are trimmed too (the `%20` keeps them off the end of the string for `Trim`).
        ("/%41#x\r%20", "/A#x"),
        ("/%41#x\t%20", "/A#x"),
        ("/%41#x\n%20", "/A#x"),
        ("#%41\r%20", "#A"),
        ("/%C3%A9#x\r\n\t %20", "/%C3%A9#x"),
        ("/%41#a%20", "/A#a"),
        ("/%zz#%20", "/%25zz#"),
        // ...but not without that path, not mid-fragment, not in path or query, not after a swallowed `%`.
        ("#a%20", "#a%20"),
        ("#%20", "#%20"),
        ("/%22#%20", "/%22#%20"),
        ("#\u{e9}%20b", "#%C3%A9%20b"),
        ("/\u{e9}%2e%20", "/%C3%A9.%20"),
        ("?\u{e9}%20", "?%C3%A9%20"),
        ("#%%20", "#%25%20"),
        ("/x#a\r%20", "/x#a%0D%20"),
        ("/%41#x\r%20y", "/A#x%0D%20y"),
    ];
    for &(input, expected) in rows {
        let url = format!("{P}{input}");
        let want = format!("{P}{expected}");
        assert_eq!(
            validate_update_url(Some(&url)).as_deref(),
            Some(want.as_str()),
            "{url:?}"
        );
    }
}

// C#: (no direct test) UpdateUrlPolicy.Validate, both chars after `https:` must be separators.
#[test]
fn validate_requires_two_separators_after_scheme() {
    for url in [
        "https:/Xgithub.com/ConanKLOP2/PhotoReview-rs",
        "https:X/github.com/ConanKLOP2/PhotoReview-rs",
    ] {
        assert_eq!(validate_update_url(Some(url)), None, "{url}");
    }
}

// C#: (no direct test) UpdateUrlPolicy.Validate, inputs shorter than the shortest valid `https://x` prefix. The
// 7-character `https:/` and `https:\` are the boundary: the length guard must reject them before indexing byte 7
// (a guard one too small would read past the end).
#[test]
fn validate_rejects_inputs_too_short_for_scheme_and_two_separators() {
    for url in [
        "",
        "h",
        "https",
        "https:",
        "https:/",
        "https:\\",
        "https:/\\",
        "https://",
    ] {
        assert_eq!(validate_update_url(Some(url)), None, "{url:?}");
    }
}

// C#: (no direct test) UpdateUrlPolicy.Validate, bidi controls in the host. .NET drops LRM, RLM and LRE..RLO
// (`UriHelper.IsBidiControlCharacter`) from the host only; other format chars and the isolates are not dropped.
// Every expected value below was produced by the C# UpdateUrlPolicy.Validate (e441d67a) on .NET 10.
#[test]
fn validate_strips_bidi_controls_from_host() {
    let accepted: &[(&str, &str)] = &[
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs",
            "https://github.com/ConanKLOP2/PhotoReview-rs",
        ),
        (
            "https://github.com\u{202E}/ConanKLOP2/PhotoReview-rs/releases",
            "https://github.com/ConanKLOP2/PhotoReview-rs/releases",
        ),
        (
            "https://\u{200F}GitHub.Com\u{202A}:443/ConanKLOP2/PhotoReview-rs?q#f",
            "https://github.com/ConanKLOP2/PhotoReview-rs?q#f",
        ),
        (
            "https://@github\u{202B}\u{202C}\u{202D}.com/ConanKLOP2/PhotoReview-rs",
            "https://@github.com/ConanKLOP2/PhotoReview-rs",
        ),
    ];
    for &(url, expected) in accepted {
        assert_eq!(validate_update_url(Some(url)).as_deref(), Some(expected), "{url:?}");
    }
    for url in [
        "https://git\u{061C}hub.com/ConanKLOP2/PhotoReview-rs",
        "https://git\u{2066}hub.com/ConanKLOP2/PhotoReview-rs",
        "https://git\u{2069}hub.com/ConanKLOP2/PhotoReview-rs",
        "https://git\u{200B}hub.com/ConanKLOP2/PhotoReview-rs",
        "https://\u{200E}@github.com/ConanKLOP2/PhotoReview-rs",
        "https://github.com:443\u{200E}/ConanKLOP2/PhotoReview-rs",
        "https://github.com:4\u{200E}43/ConanKLOP2/PhotoReview-rs",
        "https://\u{200E}/ConanKLOP2/PhotoReview-rs",
        "https://evil\u{200E}.example/ConanKLOP2/PhotoReview-rs",
    ] {
        assert_eq!(validate_update_url(Some(url)), None, "{url:?}");
    }
    // Outside the host a bidi control is an ordinary non-ASCII char.
    assert_eq!(
        validate_update_url(Some("https://github.com/ConanKLOP2/PhotoReview-rs/\u{200E}")).as_deref(),
        Some("https://github.com/ConanKLOP2/PhotoReview-rs/%E2%80%8E")
    );
}

// C#: (no direct test) UpdateUrlPolicy.Validate, a bidi control dropped from the host still makes the URI
// non-ASCII, so .NET takes its IRI rewrite path and trims the fragment's trailing space/tab/LF/CR (also a space
// decoded from %20). The same URL with a plain ASCII host keeps the fragment as typed. Every expected value below
// was produced by the C# UpdateUrlPolicy.Validate (e441d67a) on .NET 10.
#[test]
fn validate_bidi_host_trims_fragment_like_dotnet() {
    let cases: &[(&str, Option<&str>)] = &[
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs#a"),
        ),
        (
            "https://github.com/ConanKLOP2/PhotoReview-rs#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs#a%20"),
        ),
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs/^#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs/%5E#a"),
        ),
        (
            "https://github.com/ConanKLOP2/PhotoReview-rs/^#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs/%5E#a%20"),
        ),
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs/%00#a%20\t",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs/%00#a"),
        ),
        (
            "https://github.com/ConanKLOP2/PhotoReview-rs/%00#a%20\t",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs/%00#a%20"),
        ),
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs/}#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs/%7D#a"),
        ),
        (
            "https://github.com/ConanKLOP2/PhotoReview-rs/}#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs/%7D#a%20"),
        ),
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs#a \t",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs#a"),
        ),
        (
            "https://github.com\u{202A}/ConanKLOP2/PhotoReview-rs#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs#a"),
        ),
        (
            "https://\u{200F}github.com/ConanKLOP2/PhotoReview-rs#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs#a"),
        ),
        (
            "https://github.com\u{200E}:443/ConanKLOP2/PhotoReview-rs#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs#a"),
        ),
        (
            "https://@git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs#a%20",
            Some("https://@github.com/ConanKLOP2/PhotoReview-rs#a"),
        ),
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs?q%20#a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs?q%20#a"),
        ),
        // Only the fragment is trimmed: a trailing %20 in the query or path stays.
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs?q%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs?q%20"),
        ),
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs/a%20",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs/a%20"),
        ),
        (
            "https://git\u{200E}hub.com/ConanKLOP2/PhotoReview-rs/%2fx#%5c",
            Some("https://github.com/ConanKLOP2/PhotoReview-rs/%2fx#%5c"),
        ),
        // A bidi control in the port or before the `@` is not dropped.
        ("https://github.com:4\u{200E}43/ConanKLOP2/PhotoReview-rs#a%20", None),
        ("https://github.com:\u{200E}443/ConanKLOP2/PhotoReview-rs#a%20", None),
        ("https://\u{200E}@github.com/ConanKLOP2/PhotoReview-rs#a%20", None),
    ];
    for &(url, expected) in cases {
        assert_eq!(validate_update_url(Some(url)).as_deref(), expected, "{url:?}");
    }
}

// C#: (no direct test) UpdateUrlPolicy.Validate IRI character range edges, expected strings
// produced by running the exact C# Validate body on .NET 10. Columns: code point, literal char in
// query, literal char in path, %-escaped (lowercase) in query, %-escaped (lowercase) in path.
#[test]
fn validate_iri_char_range_edges_match_dotnet() {
    const BASE: &str = "https://github.com/ConanKLOP2/PhotoReview-rs";
    let rows: &[(u32, &str, &str, &str, &str)] = &[
        (0x9F, "?q=a%C2%9Fb", "/a%C2%9Fb", "?q=%c2%9f", "/a%c2%9fb"),
        (0xA0, "?q=a%C2%A0b", "/a%C2%A0b", "?q=%C2%A0", "/a%C2%A0b"),
        (0xD7FF, "?q=a%ED%9F%BFb", "/a%ED%9F%BFb", "?q=%ED%9F%BF", "/a%ED%9F%BFb"),
        (0xE000, "?q=a%EE%80%80b", "/a%EE%80%80b", "?q=%EE%80%80", "/a%ee%80%80b"),
        (0xF8FF, "?q=a%EF%A3%BFb", "/a%EF%A3%BFb", "?q=%EF%A3%BF", "/a%ef%a3%bfb"),
        (0xF900, "?q=a%EF%A4%80b", "/a%EF%A4%80b", "?q=%EF%A4%80", "/a%EF%A4%80b"),
        (0xFDCF, "?q=a%EF%B7%8Fb", "/a%EF%B7%8Fb", "?q=%EF%B7%8F", "/a%EF%B7%8Fb"),
        (0xFDD0, "?q=a%EF%B7%90b", "/a%EF%B7%90b", "?q=%ef%b7%90", "/a%ef%b7%90b"),
        (0xFDEF, "?q=a%EF%B7%AFb", "/a%EF%B7%AFb", "?q=%ef%b7%af", "/a%ef%b7%afb"),
        (0xFDF0, "?q=a%EF%B7%B0b", "/a%EF%B7%B0b", "?q=%EF%B7%B0", "/a%EF%B7%B0b"),
        (0xFFEF, "?q=a%EF%BF%AFb", "/a%EF%BF%AFb", "?q=%EF%BF%AF", "/a%EF%BF%AFb"),
        (0xFFF0, "?q=a%EF%BF%B0b", "/a%EF%BF%B0b", "?q=%ef%bf%b0", "/a%ef%bf%b0b"),
        (0xFFFD, "?q=a%EF%BF%BDb", "/a%EF%BF%BDb", "?q=%ef%bf%bd", "/a%ef%bf%bdb"),
        (0xFFFE, "?q=a%EF%BF%BEb", "/a%EF%BF%BEb", "?q=%ef%bf%be", "/a%ef%bf%beb"),
        (0xFFFF, "?q=a%EF%BF%BFb", "/a%EF%BF%BFb", "?q=%ef%bf%bf", "/a%ef%bf%bfb"),
        (
            0x10000,
            "?q=a%F0%90%80%80b",
            "/a%F0%90%80%80b",
            "?q=%F0%90%80%80",
            "/a%F0%90%80%80b",
        ),
        (
            0x1FFFD,
            "?q=a%F0%9F%BF%BDb",
            "/a%F0%9F%BF%BDb",
            "?q=%F0%9F%BF%BD",
            "/a%F0%9F%BF%BDb",
        ),
        (
            0x1FFFE,
            "?q=a%F0%9F%BF%BEb",
            "/a%F0%9F%BF%BEb",
            "?q=%f0%9f%bf%be",
            "/a%f0%9f%bf%beb",
        ),
        (
            0xE0000,
            "?q=a%F3%A0%80%80b",
            "/a%F3%A0%80%80b",
            "?q=%f3%a0%80%80",
            "/a%f3%a0%80%80b",
        ),
        (
            0xE0FFF,
            "?q=a%F3%A0%BF%BFb",
            "/a%F3%A0%BF%BFb",
            "?q=%f3%a0%bf%bf",
            "/a%f3%a0%bf%bfb",
        ),
        (
            0xE1000,
            "?q=a%F3%A1%80%80b",
            "/a%F3%A1%80%80b",
            "?q=%F3%A1%80%80",
            "/a%F3%A1%80%80b",
        ),
        (
            0xEFFFD,
            "?q=a%F3%AF%BF%BDb",
            "/a%F3%AF%BF%BDb",
            "?q=%F3%AF%BF%BD",
            "/a%F3%AF%BF%BDb",
        ),
        (
            0xEFFFE,
            "?q=a%F3%AF%BF%BEb",
            "/a%F3%AF%BF%BEb",
            "?q=%f3%af%bf%be",
            "/a%f3%af%bf%beb",
        ),
        (
            0xF0000,
            "?q=a%F3%B0%80%80b",
            "/a%F3%B0%80%80b",
            "?q=%F3%B0%80%80",
            "/a%f3%b0%80%80b",
        ),
        (
            0xFFFFD,
            "?q=a%F3%BF%BF%BDb",
            "/a%F3%BF%BF%BDb",
            "?q=%F3%BF%BF%BD",
            "/a%f3%bf%bf%bdb",
        ),
        (
            0x100000,
            "?q=a%F4%80%80%80b",
            "/a%F4%80%80%80b",
            "?q=%F4%80%80%80",
            "/a%f4%80%80%80b",
        ),
        (
            0x10FFFD,
            "?q=a%F4%8F%BF%BDb",
            "/a%F4%8F%BF%BDb",
            "?q=%F4%8F%BF%BD",
            "/a%f4%8f%bf%bdb",
        ),
        (
            0x10FFFE,
            "?q=a%F4%8F%BF%BEb",
            "/a%F4%8F%BF%BEb",
            "?q=%f4%8f%bf%be",
            "/a%f4%8f%bf%beb",
        ),
    ];
    for &(cp, q_lit, p_lit, q_esc, p_esc) in rows {
        let ch = char::from_u32(cp).expect("scalar value");
        let mut buf = [0u8; 4];
        let esc: String = ch.encode_utf8(&mut buf).bytes().map(|b| format!("%{b:02x}")).collect();
        let cases = [
            (format!("{BASE}?q=a{ch}b"), q_lit),
            (format!("{BASE}/a{ch}b"), p_lit),
            (format!("{BASE}?q={esc}"), q_esc),
            (format!("{BASE}/a{esc}b"), p_esc),
        ];
        for (input, suffix) in cases {
            assert_eq!(
                validate_update_url(Some(&input)),
                Some(format!("{BASE}{suffix}")),
                "U+{cp:04X}: {input:?}"
            );
        }
    }
}

// C#: (gap) AppVersion.TryParse cuts build metadata at the FIRST '+' (IndexOf), not the last.
#[test]
fn try_parse_cuts_at_first_plus() {
    let p = v("1.2.3+a+b");
    assert_eq!((p.major, p.minor, p.patch, p.is_prerelease), (1, 2, 3, false));
    assert_eq!(p.to_string(), "1.2.3");
    let p = v("1.2.3+a-b+c");
    assert_eq!((p.major, p.minor, p.patch, p.is_prerelease), (1, 2, 3, false));
    // A leading '+' (or '+' only) leaves an empty string: one part, not three.
    for text in ["+1.2.3", "+", "v+1.2.3", "1.2+3.4"] {
        assert_eq!(AppVersion::try_parse(Some(text)), None, "{text:?}");
    }
}

// C#: (gap) UpdateUrlPolicy.Validate userinfo handling; every None here is Uri.TryCreate == false or UserInfo non-empty.
#[test]
fn validate_userinfo_cases_match_dotnet() {
    assert_eq!(
        validate_update_url(Some("https://github.com/ConanKLOP2/PhotoReview-rs")).as_deref(),
        Some("https://github.com/ConanKLOP2/PhotoReview-rs")
    );
    assert_eq!(
        validate_update_url(Some("https://@github.com/ConanKLOP2/PhotoReview-rs")).as_deref(),
        Some("https://@github.com/ConanKLOP2/PhotoReview-rs")
    );
    for url in [
        "https://@github.com@/ConanKLOP2/PhotoReview-rs",
        "https://@@github.com/ConanKLOP2/PhotoReview-rs",
        "https://a@b@github.com/ConanKLOP2/PhotoReview-rs",
        "https://github.com@/ConanKLOP2/PhotoReview-rs",
        "https://@github.com@evil.com/ConanKLOP2/PhotoReview-rs",
    ] {
        assert_eq!(validate_update_url(Some(url)), None, "{url}");
    }
}
