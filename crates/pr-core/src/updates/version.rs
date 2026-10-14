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

    let path = remove_dot_segments(&escape_uri_part(raw_path, true));

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
        out.push_str(&escape_uri_part(q, false));
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(&escape_uri_part(f, false));
    }
    Some(out)
}

fn hex_val(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

fn push_escaped(out: &mut String, byte: u8) {
    out.push('%');
    out.push_str(&format!("{byte:02X}"));
}

/// Escapes a path (`is_path`: `\` becomes `/`) or query/fragment (`\` becomes `%5C`) the way `System.Uri` does for
/// the subset of inputs the update-URL policy sees.
fn escape_uri_part(input: &str, is_path: bool) -> String {
    let chars: Vec<char> = input.chars().collect();
    let escape_at = |i: usize| -> Option<u8> {
        if chars.get(i) == Some(&'%') {
            let hi = hex_val(*chars.get(i + 1)?)?;
            let lo = hex_val(*chars.get(i + 2)?)?;
            Some(hi * 16 + lo)
        } else {
            None
        }
    };
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '%' {
            match escape_at(i) {
                None => {
                    out.push_str("%25");
                    i += 1;
                }
                Some(b) if b < 0x80 => {
                    if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                        out.push(b as char);
                    } else {
                        out.extend(&chars[i..i + 3]);
                    }
                    i += 3;
                }
                Some(_) => {
                    let mut run: Vec<u8> = Vec::new();
                    let mut j = i;
                    while let Some(b) = escape_at(j) {
                        if b < 0x80 {
                            break;
                        }
                        run.push(b);
                        j += 3;
                    }
                    let need = match run[0] {
                        0xC2..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF4 => 4,
                        _ => 0,
                    };
                    if need > 0 && run.len() >= need && std::str::from_utf8(&run[..need]).is_ok() {
                        for &b in &run[..need] {
                            push_escaped(&mut out, b);
                        }
                        i += 3 * need;
                    } else {
                        out.extend(&chars[i..i + 3]);
                        i += 3;
                    }
                }
            }
            continue;
        }
        i += 1;
        if c == '\\' {
            if is_path {
                out.push('/');
            } else {
                out.push_str("%5C");
            }
        } else if (c as u32) <= 0x20 || c as u32 == 0x7F || matches!(c, '"' | '<' | '>' | '^' | '`' | '{' | '|' | '}') {
            push_escaped(&mut out, c as u32 as u8);
        } else if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                push_escaped(&mut out, b);
            }
        }
    }
    out
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
