//! Port of `PhotoReview.Core/Updates/AppVersion.cs`, `UpdateUrlPolicy.cs` and `UpdateCheckResult.cs`
//! (C# baseline e441d67a).
//!
//! * [`AppVersion`]: numeric `major.minor.patch` plus a pre-release flag; parses `"v2.0.93"`, `"2.0.93+hash"`,
//!   `"2.1.0-beta.1"`.
//! * [`validate_update_url`]: only `https://github.com/ConanKLOP2/PhotoReview[/...]` may be opened from the update
//!   check.
//! * [`UpdateCheckResult`]: outcome of a manual update check (constructed only through the three factory functions).
//!
//! `IUpdateChecker` / `UpdateChecker` (HTTP) are NOT part of this unit.

use std::cmp::Ordering;
use std::fmt;

/// `major.minor.patch` with an optional pre-release flag (C# `record struct AppVersion`).
///
/// Derived equality compares all four fields (like the C# record struct); ordering is [`Ord`] below and agrees with
/// equality (two values compare `Equal` only when all four fields are equal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AppVersion {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
    pub is_prerelease: bool,
}

impl AppVersion {
    /// C# `AppVersion.TryParse(string?, out AppVersion)`. `None` is the `false` result (C# leaves `default`).
    /// Accepts an optional leading `v`/`V`, optional `+build` and `-prerelease` suffixes; see contract.md.
    pub fn try_parse(text: Option<&str>) -> Option<AppVersion> {
        let text = text?;
        if text.chars().all(char::is_whitespace) {
            return None;
        }
        let mut s = text.trim();
        if let Some(rest) = s.strip_prefix(['v', 'V']) {
            s = rest;
        }
        if let Some(plus) = s.find('+') {
            s = &s[..plus];
        }
        let mut is_prerelease = false;
        if let Some(dash) = s.find('-') {
            if dash == s.len() - 1 {
                return None;
            }
            is_prerelease = true;
            s = &s[..dash];
        }
        let mut numbers = [0i32; 3];
        let mut count = 0usize;
        for part in s.split('.') {
            if count >= 3 {
                return None;
            }
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            numbers[count] = part.parse::<i32>().ok()?;
            count += 1;
        }
        if count != 3 {
            return None;
        }
        Some(AppVersion {
            major: numbers[0],
            minor: numbers[1],
            patch: numbers[2],
            is_prerelease,
        })
    }
}

impl Ord for AppVersion {
    /// Numeric major, minor, patch; at equal numbers a pre-release sorts BELOW the release.
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then(other.is_prerelease.cmp(&self.is_prerelease))
    }
}

impl PartialOrd for AppVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for AppVersion {
    /// `"{major}.{minor}.{patch}"`, invariant digits; the pre-release flag is NOT printed (C# `ToString`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// C# `UpdateUrlPolicy.ReleasesPageUrl`.
pub const RELEASES_PAGE_URL: &str = "https://github.com/ConanKLOP2/PhotoReview/releases";

/// C# `UpdateUrlPolicy.Validate(string?)`: the normalized absolute URL (what `Uri.AbsoluteUri` would return) when
/// `url` is `https://github.com/ConanKLOP2/PhotoReview` or below it, otherwise `None`.
pub fn validate_update_url(url: Option<&str>) -> Option<String> {
    let url = url?;
    if url.chars().all(char::is_whitespace) {
        return None;
    }
    let s = url.trim();

    // 1. Scheme and separators: `https:` + exactly two of `/` or `\`, then a non-separator.
    let is_sep = |c: char| c == '/' || c == '\\';
    let bytes = s.as_bytes();
    if bytes.len() < 8 || !bytes[..5].eq_ignore_ascii_case(b"https") || bytes[5] != b':' {
        return None;
    }
    if !is_sep(bytes[6] as char) || !is_sep(bytes[7] as char) {
        return None;
    }
    let rest = &s[8..];
    if rest.starts_with(is_sep) {
        return None;
    }

    // 2. Authority: up to the first `/`, `?` or `#`.
    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let after = &rest[auth_end..];
    if authority.contains('\\') {
        return None;
    }
    let mut empty_userinfo = false;
    let mut host_port = authority;
    if authority.contains('@') {
        let mut it = authority.split('@');
        let user = it.next().unwrap_or("");
        let host = it.next().unwrap_or("");
        if it.next().is_some() || !user.is_empty() {
            return None;
        }
        empty_userinfo = true;
        host_port = host;
    }
    let host = match host_port.rfind(':') {
        Some(colon) => {
            let port = &host_port[colon + 1..];
            if !port.is_empty() && (!port.bytes().all(|b| b.is_ascii_digit()) || port.trim_start_matches('0') != "443")
            {
                return None;
            }
            &host_port[..colon]
        }
        None => host_port,
    };
    // .NET drops the bidi formatting controls (`UriHelper.IsBidiControlCharacter`) from the host before it checks
    // the name: `https://git\u{200E}hub.com/...` is `github.com`. Such a host is non-ASCII, so .NET also takes its
    // IRI rewrite path for the whole URI (it trims the fragment's trailing white space, see `escape_uri_part`).
    let host_had_bidi = host.chars().any(is_bidi_control);
    let host: String = host.chars().filter(|&c| !is_bidi_control(c)).collect();
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }

    // 3/4. Split path, query, fragment.
    let (raw_path, query_frag) = match after.find(['?', '#']) {
        Some(i) => (&after[..i], &after[i..]),
        None => (after, ""),
    };
    let raw_path = if raw_path.is_empty() { "/" } else { raw_path };
    let mut query: Option<&str> = None;
    let mut fragment: Option<&str> = None;
    if let Some(q) = query_frag.strip_prefix('?') {
        match q.find('#') {
            Some(h) => {
                query = Some(&q[..h]);
                fragment = Some(&q[h + 1..]);
            }
            None => query = Some(q),
        }
    } else if let Some(f) = query_frag.strip_prefix('#') {
        fragment = Some(f);
    }
    let iri_rewrite = host_had_bidi || needs_iri_rewrite(after);

    let path = remove_dot_segments(&escape_uri_part(raw_path, Part::Path, iri_rewrite));

    // 5. Allow-list.
    const PROJECT: &str = "/ConanKLOP2/PhotoReview";
    let ok = path.eq_ignore_ascii_case(PROJECT)
        || (path.len() > PROJECT.len()
            && path.as_bytes()[..PROJECT.len()].eq_ignore_ascii_case(PROJECT.as_bytes())
            && path.as_bytes()[PROJECT.len()] == b'/');
    if !ok {
        return None;
    }

    // 6. Result.
    let mut out = String::from("https://");
    if empty_userinfo {
        out.push('@');
    }
    out.push_str("github.com");
    out.push_str(&path);
    if let Some(q) = query {
        out.push('?');
        out.push_str(&escape_uri_part(q, Part::Query, iri_rewrite));
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(&escape_uri_part(f, Part::Fragment, iri_rewrite));
    }
    Some(out)
}

/// Hex value of a UTF-16 code unit that is an ASCII hex digit (either case).
fn hex_val(unit: u16) -> Option<u8> {
    char::from_u32(u32::from(unit))
        .and_then(|c| c.to_digit(16))
        .map(|d| d as u8)
}

/// The byte encoded by `%XX` at `units[i..i + 3]`, if those three units are `%` and two hex digits.
fn escape_at(units: &[u16], i: usize) -> Option<u8> {
    if units.get(i) != Some(&u16::from(b'%')) {
        return None;
    }
    let hi = hex_val(*units.get(i + 1)?)?;
    let lo = hex_val(*units.get(i + 2)?)?;
    Some(hi * 16 + lo)
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

fn push_escaped(out: &mut String, byte: u8) {
    out.push('%');
    out.push_str(&format!("{byte:02X}"));
}

const REPLACEMENT: u16 = 0xFFFD;

/// Pass 1 (`IriHelper.EscapeUnescapeIri`), on UTF-16 code units like .NET: decodes `%XX` escapes of ASCII
/// unreserved chars and of complete, valid UTF-8 sequences; keeps every other escape verbatim. A `%` with at least
/// two more units after it that does NOT start a valid escape is copied TOGETHER WITH the next two units, unexamined
/// (so in `%%2E` the `%2E` is never decoded, and in `%%<emoji>` the high surrogate is split from its low half).
/// A `%` with fewer than two units after it is copied alone. Lone surrogates become U+FFFD.
fn unescape_iri(units: &[u16], is_query: bool) -> Vec<u16> {
    let n = units.len();
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let c = units[i];
        if c == u16::from(b'%') {
            if n - i <= 2 {
                out.push(c);
                i += 1;
                continue;
            }
            match escape_at(units, i) {
                // `%20` is decoded too; pass 2 re-escapes the space, so it only matters for the fragment trim.
                Some(b) if b < 0x80 && (is_unreserved(b) || b == b' ') => {
                    out.push(u16::from(b));
                    i += 3;
                }
                Some(b) if b >= 0x80 => {
                    let mut run: Vec<u8> = Vec::new();
                    let mut j = i;
                    while let Some(rb) = escape_at(units, j) {
                        if rb < 0x80 {
                            break;
                        }
                        run.push(rb);
                        j += 3;
                    }
                    let need = match run[0] {
                        0xC2..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF4 => 4,
                        _ => 0,
                    };
                    let decoded = run.get(..need).and_then(|seq| std::str::from_utf8(seq).ok());
                    match decoded {
                        Some(text) if need > 0 && text.chars().all(|ch| is_iri_char(ch, is_query)) => {
                            out.extend(text.encode_utf16());
                            i += 3 * need;
                        }
                        _ => {
                            out.extend_from_slice(&units[i..i + 3]);
                            i += 3;
                        }
                    }
                }
                _ => {
                    // Not a valid escape, or an ASCII escape that is not unreserved: three units verbatim.
                    out.extend_from_slice(&units[i..i + 3]);
                    i += 3;
                }
            }
            continue;
        }
        if is_high_surrogate(c) && units.get(i + 1).is_some_and(|&d| is_low_surrogate(d)) {
            out.extend_from_slice(&units[i..i + 2]);
            i += 2;
        } else if is_high_surrogate(c) || is_low_surrogate(c) {
            out.push(REPLACEMENT);
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// `IriHelper.CheckIriUnicodeRange` (RFC 3987 `ucschar`, plus `iprivate` in a query) minus the bidi controls that
/// .NET never unescapes: only such chars are decoded from a UTF-8 escape sequence; the rest stay escaped verbatim.
fn is_iri_char(ch: char, is_query: bool) -> bool {
    let cp = u32::from(ch);
    if cp < 0x10000 {
        return matches!(cp, 0xA0..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFEF)
            || (is_query && (0xE000..=0xF8FF).contains(&cp));
    }
    let in_plane = cp & 0xFFFF;
    if in_plane >= 0xFFFE {
        return false;
    }
    cp <= 0xEFFFD && !(0xE0000..=0xE0FFF).contains(&cp) || (is_query && cp >= 0xF0000)
}

/// .NET `UriHelper.IsBidiControlCharacter`: LRM, RLM and LRE..RLO (NOT U+061C or the U+2066..U+2069 isolates).
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}')
}

fn is_high_surrogate(u: u16) -> bool {
    (0xD800..=0xDBFF).contains(&u)
}

fn is_low_surrogate(u: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&u)
}

/// Pass 2 (`UriHelper.EscapeString` with `checkExistingEscaped`): keeps valid `%XX` escapes verbatim, escapes a
/// stray `%` as `%25`, escapes control/space/unsafe ASCII and every non-ASCII char (UTF-8, uppercase hex; a lone
/// surrogate as U+FFFD). In a path (`is_path`) `\` becomes `/`, elsewhere it is escaped as `%5C`.
fn escape_units(units: &[u16], is_path: bool) -> String {
    let n = units.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;
    while i < n {
        let c = units[i];
        if c == u16::from(b'%') {
            if escape_at(units, i).is_some() {
                for &u in &units[i..i + 3] {
                    out.push(char::from(u as u8));
                }
                i += 3;
            } else {
                out.push_str("%25");
                i += 1;
            }
            continue;
        }
        if c < 0x80 {
            let b = c as u8;
            if b == b'\\' {
                if is_path {
                    out.push('/');
                } else {
                    out.push_str("%5C");
                }
            } else if b <= 0x20 || b == 0x7F || matches!(b, b'"' | b'<' | b'>' | b'^' | b'`' | b'{' | b'|' | b'}') {
                push_escaped(&mut out, b);
            } else {
                out.push(char::from(b));
            }
            i += 1;
            continue;
        }
        let (ch, used) = if is_high_surrogate(c) && units.get(i + 1).is_some_and(|&d| is_low_surrogate(d)) {
            let hi = u32::from(c - 0xD800);
            let lo = u32::from(units[i + 1] - 0xDC00);
            (char::from_u32(0x10000 + (hi << 10) + lo), 2)
        } else {
            (char::from_u32(u32::from(c)), 1)
        };
        let ch = ch.unwrap_or(char::REPLACEMENT_CHARACTER);
        let mut buf = [0u8; 4];
        for &b in ch.encode_utf8(&mut buf).as_bytes() {
            push_escaped(&mut out, b);
        }
        i += used;
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Path,
    Query,
    Fragment,
}

/// Whether .NET takes its IRI rewrite path for this URI (`raw` = everything after the authority): it holds a
/// non-ASCII char, or a `%` that is not the start of an escape of a reserved/unsafe ASCII char (an unreserved-char
/// escape, an escape of a byte >= 0x80, or a `%` without two hex digits after it). Only that path trims the
/// fragment's trailing white space (see [`escape_uri_part`]).
fn needs_iri_rewrite(raw: &str) -> bool {
    let units: Vec<u16> = raw.encode_utf16().collect();
    units.iter().enumerate().any(|(i, &u)| {
        u >= 0x80
            || (u == u16::from(b'%')
                && match escape_at(&units, i) {
                    Some(b) => b >= 0x80 || is_unreserved(b),
                    None => true,
                })
    })
}

/// Escapes a path, query or fragment the way `System.Uri.AbsoluteUri` does for the inputs the update-URL policy
/// sees: [`unescape_iri`] then [`escape_units`]. On the IRI rewrite path (`iri_rewrite`) .NET also drops the
/// fragment's trailing space, tab, LF and CR chars (literal, or a space decoded from `%20`): `#%41%20` gives `#A`
/// while `#a%20` (no rewrite) keeps `#a%20`.
fn escape_uri_part(input: &str, part: Part, iri_rewrite: bool) -> String {
    let units: Vec<u16> = input.encode_utf16().collect();
    let mut unescaped = unescape_iri(&units, part == Part::Query);
    if iri_rewrite && part == Part::Fragment {
        while unescaped
            .last()
            .is_some_and(|&u| matches!(u, 0x20 | 0x09 | 0x0A | 0x0D))
        {
            unescaped.pop();
        }
    }
    escape_units(&unescaped, part == Part::Path)
}

/// RFC 3986 dot-segment removal on an absolute path (starts with `/`).
fn remove_dot_segments(path: &str) -> String {
    let segs: Vec<&str> = path.strip_prefix('/').unwrap_or(path).split('/').collect();
    let mut out: Vec<&str> = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        let last = i + 1 == segs.len();
        match *seg {
            "." => {
                if last {
                    out.push("");
                }
            }
            ".." => {
                out.pop();
                if last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    format!("/{}", out.join("/"))
}

/// C# `UpdateCheckStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UpdateCheckStatus {
    #[default]
    UpToDate,
    UpdateAvailable,
    Failed,
}

/// C# `UpdateFailure`: why a manual update check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UpdateFailure {
    #[default]
    None,
    Offline,
    Timeout,
    RateLimited,
    BadResponse,
    InvalidVersion,
}

/// C# `sealed record UpdateCheckResult`. Fields are private (C# `private init`); build with
/// [`up_to_date`](Self::up_to_date), [`update_available`](Self::update_available) or [`failed`](Self::failed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckResult {
    status: UpdateCheckStatus,
    latest_version: Option<String>,
    download_url: Option<String>,
    failure: UpdateFailure,
}

impl UpdateCheckResult {
    /// Status `UpToDate`, `latest_version` set, no URL, failure `None`.
    pub fn up_to_date(latest_version: &str) -> Self {
        Self {
            status: UpdateCheckStatus::UpToDate,
            latest_version: Some(latest_version.to_owned()),
            download_url: None,
            failure: UpdateFailure::None,
        }
    }

    /// Status `UpdateAvailable`, `latest_version` and `download_url` set (stored verbatim, not re-validated),
    /// failure `None`.
    pub fn update_available(latest_version: &str, download_url: &str) -> Self {
        Self {
            status: UpdateCheckStatus::UpdateAvailable,
            latest_version: Some(latest_version.to_owned()),
            download_url: Some(download_url.to_owned()),
            failure: UpdateFailure::None,
        }
    }

    /// Status `Failed`, `failure` set, no version and no URL.
    pub fn failed(failure: UpdateFailure) -> Self {
        Self {
            status: UpdateCheckStatus::Failed,
            latest_version: None,
            download_url: None,
            failure,
        }
    }

    pub fn status(&self) -> UpdateCheckStatus {
        self.status
    }

    /// Latest published version (numeric `major.minor.patch`) for `UpToDate`/`UpdateAvailable`, else `None`.
    pub fn latest_version(&self) -> Option<&str> {
        self.latest_version.as_deref()
    }

    /// Download page for `UpdateAvailable`, else `None`.
    pub fn download_url(&self) -> Option<&str> {
        self.download_url.as_deref()
    }

    pub fn failure(&self) -> UpdateFailure {
        self.failure
    }
}
