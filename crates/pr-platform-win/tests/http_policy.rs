//! W10-H1 pure policy tables; runtime defaults still require a .NET loopback probe.
use pr_core::updates::checker::HttpRequestError as E;
use pr_platform_win::http::policy::*;
use std::time::Duration;

#[test]
fn redirect_decision_status_location_limit_and_downgrade_returns_expected_decision() {
    // New: no C# redirect-policy test; UpdateChecker.cs:43 handles the returned 3xx.
    for status in [
        200, 299, 300, 301, 302, 303, 304, 305, 306, 307, 308, 309, 403, 429, 500,
    ] {
        for from in [false, true] {
            for to in [false, true] {
                for location in [false, true] {
                    for followed in [0, 1, 48, 49, 50, 51, u32::MAX] {
                        let redirect = [301, 302, 303, 307, 308].contains(&status);
                        let expected = if redirect && location && followed <= 49 && !(from && !to) {
                            RedirectDecision::Follow
                        } else {
                            RedirectDecision::ReturnResponse
                        };
                        assert_eq!(
                            redirect_decision(status, from, to, location, followed),
                            expected,
                            "{status} {from} {to} {location} {followed}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn proxy_source_all_boolean_combinations_selects_expected_source() {
    // New: no C# test of default proxy selection.
    use ProxySource::*;
    for (https, all, user, expected) in [
        (false, false, false, Direct),
        (false, false, true, UserSettings),
        (false, true, false, AllEnvironment),
        (false, true, true, AllEnvironment),
        (true, false, false, HttpsEnvironment),
        (true, false, true, HttpsEnvironment),
        (true, true, false, HttpsEnvironment),
        (true, true, true, HttpsEnvironment),
    ] {
        assert_eq!(proxy_source(https, all, user), expected);
    }
}

#[test]
fn remaining_timeout_before_at_and_after_deadline_returns_expected_value() {
    // C#: UpdateCheckerBoundaryTests.CustomTimeout_IsHonoured (deadline analogue).
    let total = Duration::from_secs(10);
    for elapsed in [
        Duration::ZERO,
        Duration::from_nanos(1),
        Duration::from_secs(9),
        total - Duration::from_nanos(1),
    ] {
        assert_eq!(remaining_timeout(total, elapsed), Some(total - elapsed));
    }
    for elapsed in [total, total + Duration::from_nanos(1), Duration::MAX] {
        assert_eq!(remaining_timeout(total, elapsed), None);
    }
    for elapsed in [Duration::ZERO, total, Duration::MAX] {
        assert_eq!(remaining_timeout(Duration::ZERO, elapsed), None);
    }
}

#[test]
fn classify_winhttp_error_known_and_unknown_maps_expected_category() {
    // C#: UpdateCheckerGapTests.HttpRequestError_IsClassified (category analogue).
    let known = [
        (12007, E::NameResolutionError),
        (12029, E::ConnectionError),
        (12037, E::SecureConnectionError),
        (12038, E::SecureConnectionError),
        (12044, E::SecureConnectionError),
        (12045, E::SecureConnectionError),
        (12057, E::SecureConnectionError),
        (12157, E::SecureConnectionError),
        (12169, E::SecureConnectionError),
        (12170, E::SecureConnectionError),
        (12175, E::SecureConnectionError),
        (12179, E::SecureConnectionError),
        (12152, E::InvalidResponse),
        (12181, E::ConfigurationLimitExceeded),
        (12182, E::ConfigurationLimitExceeded),
        (12183, E::ConfigurationLimitExceeded),
        (12184, E::ConfigurationLimitExceeded),
    ];
    for (code, expected) in known {
        assert_eq!(classify_winhttp_error(code), expected, "{code}");
    }
    for code in [0, 5, 12002, 12017, 12030, 12156, 12190, u32::MAX] {
        assert_eq!(classify_winhttp_error(code), E::Unknown, "{code}");
    }
}
