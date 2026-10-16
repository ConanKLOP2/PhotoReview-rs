//! Port of `tests/PhotoReview.Imaging.Tests/Decoding/WicCodecAvailabilityTests.cs` (C# baseline d92d6aae, 7 tests through
//! the seams) and of the one native test `WebpHeicNativeTests.Probe_OnThisPc_EnumeratesTheBuiltInDecoders`, plus the
//! golden `wic-codec-probe` (`tests/golden/wic-codec-probe.json`; its inputs in `tests/golden-inputs/wic-codec-probe.json`).
//!
//! Port of `PhotoReview.Imaging/Decoding/Wic/WicCodecAvailability.cs` (C# baseline d92d6aae).
//!
//! Red tests (W4-11b): they compile against the stub and fail through `todo!()` until the bodies are implemented.
//! Tests that only touch already-written pure data (`supports_non_optional_format_is_always_true`,
//! `container_guids_match_wincodec_h`, `golden_wic_codec_probe_constants_match_textual_guids`) pass against the stub on purpose.

#![cfg(windows)]

use std::cell::RefCell;
use std::io;

use pr_core::catalog::explorer_types::Guid;
use pr_core::catalog::types::WicImageFormat;
use pr_platform_win::wic::availability::{
    CONTAINER_FORMAT_HEIF, CONTAINER_FORMAT_WEBP, DecoderEntry, MEDIA_SUBTYPE_HEVC, WicCodecSupport, current, native,
    probe,
};
use pr_test_support::golden::{self, repo_root};
use serde_json::Value;

/// C# test-local JPEG container GUID (`WebpHeicNativeTests`), always built into WIC.
const JPEG_TEXT: &str = "19e4a5aa-5662-4fc5-a0c0-1758028e1057";

/// Textual GUID -> .NET `Guid.ToByteArray()` order, as C# `new Guid(text)` builds it: the first three groups are
/// little-endian, the last two are copied as written.
fn guid_of(text: &str) -> Guid {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    assert_eq!(hex.len(), 32, "bad GUID text {text}");
    let byte = |i: usize| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex digit pair");
    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().take(4).enumerate() {
        *slot = byte(3 - i);
    }
    for i in 0..2 {
        out[4 + i] = byte(5 - i);
    }
    for i in 0..2 {
        out[6 + i] = byte(7 - i);
    }
    for (i, slot) in out.iter_mut().enumerate().skip(8) {
        *slot = byte(i);
    }
    Guid(out)
}

/// The inverse of [`guid_of`]: a GUID as lowercase textual form (the golden's spelling).
fn guid_text(guid: &Guid) -> String {
    let b = guid.0;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// C# `WicCodecAvailability.DecoderEntry(container, "name", ".ext")`.
fn entry(container: Guid) -> DecoderEntry {
    DecoderEntry {
        container_format: container,
        friendly_name: "name".to_owned(),
        file_extensions: ".ext".to_owned(),
    }
}

// C#: WicCodecAvailabilityTests.Probe_WebpDecoderRegistered_ReportsWebpOnly
#[test]
fn probe_webp_decoder_registered_reports_webp_only() {
    let result = probe(
        || Ok(vec![entry(guid_of(JPEG_TEXT)), entry(CONTAINER_FORMAT_WEBP)]),
        |_| Ok(true),
    );

    assert!(result.webp);
    assert!(!result.heif_container);
    assert!(!result.heif()); // an HEVC decoder alone is not a HEIF codec
    assert!(result.supports(WicImageFormat::WebP));
    assert!(!result.supports(WicImageFormat::Heif));
}

// C#: WicCodecAvailabilityTests.Probe_HeifDecoderWithoutHevc_IsNotEnoughForHeic
#[test]
fn probe_heif_decoder_without_hevc_is_not_enough_for_heic() {
    let asked: RefCell<Option<Guid>> = RefCell::new(None);
    let result = probe(
        || Ok(vec![entry(CONTAINER_FORMAT_HEIF)]),
        |subtype| {
            *asked.borrow_mut() = Some(*subtype);
            Ok(false)
        },
    );

    assert!(result.heif_container);
    assert!(!result.hevc_decoder);
    assert!(!result.heif());
    assert!(!result.webp);
    assert_eq!(*asked.borrow(), Some(MEDIA_SUBTYPE_HEVC));
    assert!(
        result.detail.contains("HEVC video decoder: not found"),
        "detail: {}",
        result.detail
    );
}

// C#: WicCodecAvailabilityTests.Probe_HeifDecoderAndHevc_SupportsHeic
#[test]
fn probe_heif_decoder_and_hevc_supports_heic() {
    let result = probe(|| Ok(vec![entry(CONTAINER_FORMAT_HEIF)]), |_| Ok(true));

    assert!(result.heif());
    assert!(result.supports(WicImageFormat::Heif));
}

// C#: WicCodecAvailabilityTests.Probe_EnumerationThrows_MeansNothingAvailable_NotACrash
#[test]
fn probe_enumeration_throws_means_nothing_available_not_a_crash() {
    let result = probe(|| Err(io::Error::other("no factory")), |_| Ok(true));

    assert!(!result.webp);
    assert!(!result.heif_container);
    assert!(!result.hevc_decoder);
    assert!(result.detail.contains("no factory"), "detail: {}", result.detail);
}

// C#: WicCodecAvailabilityTests.Probe_HevcProbeThrows_MeansNoHevc_ButKeepsWebp
#[test]
fn probe_hevc_probe_throws_means_no_hevc_but_keeps_webp() {
    let result = probe(
        || Ok(vec![entry(CONTAINER_FORMAT_WEBP), entry(CONTAINER_FORMAT_HEIF)]),
        |_| Err(io::Error::other("mfplat")),
    );

    assert!(result.webp);
    assert!(result.heif_container);
    assert!(!result.heif());
    assert!(result.detail.contains("mfplat"), "detail: {}", result.detail);
}

// C#: WicCodecAvailabilityTests.Supports_NonOptionalFormat_IsAlwaysTrue
#[test]
fn supports_non_optional_format_is_always_true() {
    assert!(WicCodecSupport::none("x").supports(WicImageFormat::None));
}

// C#: WicCodecAvailabilityTests.ContainerGuids_MatchWincodecH
#[test]
fn container_guids_match_wincodec_h() {
    assert_eq!(CONTAINER_FORMAT_WEBP, guid_of("e094b0e2-67f2-45b3-b0ea-115337ca7cf3"));
    assert_eq!(CONTAINER_FORMAT_HEIF, guid_of("e1e62521-6787-405b-a339-500715b5763f"));
    assert_eq!(MEDIA_SUBTYPE_HEVC, guid_of("43564548-0000-0010-8000-00aa00389b71"));
}

// C#: WebpHeicNativeTests.Probe_OnThisPc_EnumeratesTheBuiltInDecoders (native: needs the real WIC on this PC)
#[test]
#[ignore = "native: needs WIC"]
fn probe_on_this_pc_enumerates_the_built_in_decoders() {
    let decoders = native::enumerate_decoders().expect("WIC decoder enumeration on this PC");

    assert!(
        decoders.iter().any(|d| d.container_format == guid_of(JPEG_TEXT)),
        "JPEG container must be listed"
    );
    let webp_listed = decoders.iter().any(|d| d.container_format == CONTAINER_FORMAT_WEBP);
    assert_eq!(webp_listed, current().webp);
    assert!(!current().detail.is_empty());
}

/// Reads `tests/golden-inputs/<name>.json`: the decoder list and seam answers that produced each golden row.
fn golden_inputs(name: &str) -> Vec<Value> {
    let path = repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

/// The text of a seam answer: `found`, `missing` or `throws:<message>`.
fn seam_answer(text: &str) -> io::Result<bool> {
    match text {
        "found" => Ok(true),
        "missing" => Ok(false),
        other => match other.strip_prefix("throws:") {
            Some(message) => Err(io::Error::other(message.to_owned())),
            None => panic!("unknown seam answer {other:?}"),
        },
    }
}

// Golden-driven: every row of `tests/golden/wic-codec-probe.json` (inputs from `tests/golden-inputs/`).
#[test]
fn golden_wic_codec_probe_matches_csharp_for_every_case() {
    let golden = golden::load("wic-codec-probe");
    let inputs = golden_inputs("wic-codec-probe");

    for expected in golden.cases.iter().filter(|c| c["name"] != "constants") {
        let name = expected["name"].as_str().expect("golden row has a name");
        let input = inputs
            .iter()
            .find(|i| i["name"] == name)
            .unwrap_or_else(|| panic!("no golden-inputs row named {name}"));

        let decoders: Vec<DecoderEntry> = input["decoders"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: no decoders list"))
            .iter()
            .map(|d| DecoderEntry {
                container_format: guid_of(d["guid"].as_str().expect("guid text")),
                friendly_name: d["name"].as_str().unwrap_or("").to_owned(),
                file_extensions: d["ext"].as_str().unwrap_or("").to_owned(),
            })
            .collect();
        let enumerate_failure = input.get("enumerate").map(|v| {
            v.as_str()
                .and_then(|s| s.strip_prefix("throws:"))
                .unwrap_or_else(|| panic!("{name}: bad enumerate"))
                .to_owned()
        });
        let hevc_answer = input["hevc"].as_str().expect("hevc answer").to_owned();

        let asked: RefCell<Option<Guid>> = RefCell::new(None);
        let actual = probe(
            move || match enumerate_failure {
                Some(message) => Err(io::Error::other(message)),
                None => Ok(decoders),
            },
            |subtype| {
                *asked.borrow_mut() = Some(*subtype);
                seam_answer(&hevc_answer)
            },
        );
        let asked_text = asked.borrow().as_ref().map(guid_text);

        assert_eq!(
            actual.webp,
            expected["webp"].as_bool().expect("webp"),
            "case {name}: webp"
        );
        assert_eq!(
            actual.heif_container,
            expected["heifContainer"].as_bool().expect("heifContainer"),
            "case {name}: heifContainer"
        );
        assert_eq!(
            actual.hevc_decoder,
            expected["hevcDecoder"].as_bool().expect("hevcDecoder"),
            "case {name}: hevcDecoder"
        );
        assert_eq!(
            actual.heif(),
            expected["heif"].as_bool().expect("heif"),
            "case {name}: heif"
        );
        assert_eq!(
            actual.detail,
            expected["detail"].as_str().expect("detail"),
            "case {name}: detail"
        );
        assert_eq!(
            asked_text.as_deref(),
            expected["hevcAskedFor"].as_str(),
            "case {name}: hevcAskedFor"
        );
        assert_eq!(
            actual.supports(WicImageFormat::None),
            expected["supports"]["none"].as_bool().expect("none"),
            "case {name}: supports none"
        );
        assert_eq!(
            actual.supports(WicImageFormat::WebP),
            expected["supports"]["webp"].as_bool().expect("webp"),
            "case {name}: supports webp"
        );
        assert_eq!(
            actual.supports(WicImageFormat::Heif),
            expected["supports"]["heif"].as_bool().expect("heif"),
            "case {name}: supports heif"
        );
    }
}

// Golden-driven: the `constants` row names the three GUIDs textually (the C# values).
#[test]
fn golden_wic_codec_probe_constants_match_textual_guids() {
    let golden = golden::load("wic-codec-probe");
    let row = golden
        .cases
        .iter()
        .find(|c| c["name"] == "constants")
        .expect("golden wic-codec-probe has a constants row");

    assert_eq!(
        guid_text(&CONTAINER_FORMAT_WEBP),
        row["webp"].as_str().expect("webp text")
    );
    assert_eq!(
        guid_text(&CONTAINER_FORMAT_HEIF),
        row["heif"].as_str().expect("heif text")
    );
    assert_eq!(guid_text(&MEDIA_SUBTYPE_HEVC), row["hevc"].as_str().expect("hevc text"));
}
