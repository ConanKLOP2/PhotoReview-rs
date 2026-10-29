//! W8-20a: the pure `.res` writer used by `build.rs` (new tests, no C# twin).
#[path = "../build_support/res_writer.rs"]
mod res_writer;

use res_writer::*;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// `(type, id, data)` of every resource after the empty first one; checks the 4-byte alignment on the way.
fn resources(res: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < res.len() {
        assert_eq!(at % 4, 0, "header alignment");
        let size = u32_at(res, at) as usize;
        assert_eq!(u32_at(res, at + 4), 32);
        assert_eq!(u16_at(res, at + 8), 0xFFFF);
        assert_eq!(u16_at(res, at + 12), 0xFFFF);
        let (kind, id) = (u16_at(res, at + 10), u16_at(res, at + 14));
        let data = res[at + 32..at + 32 + size].to_vec();
        if at > 0 {
            out.push((kind, id, data));
        }
        at += 32 + size.div_ceil(4) * 4;
    }
    assert_eq!(at, res.len(), "padding to the end");
    out
}

fn ico(sizes: &[usize]) -> Vec<u8> {
    let mut b = vec![0, 0, 1, 0];
    b.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + sizes.len() * 16;
    for (i, &n) in sizes.iter().enumerate() {
        b.extend_from_slice(&[16 + i as u8, 16 + i as u8, 0, 0, 1, 0, 32, 0]);
        b.extend_from_slice(&(n as u32).to_le_bytes());
        b.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += n;
    }
    for (i, &n) in sizes.iter().enumerate() {
        b.extend(std::iter::repeat_n(i as u8 + 1, n));
    }
    b
}

#[test]
fn res_starts_with_the_empty_32_byte_header() {
    let res = build_res("m", None).unwrap();
    assert_eq!(&res[..4], &[0; 4]);
    assert_eq!(u32_at(&res, 4), 32);
    assert_eq!(&res[8..12], &[0xFF, 0xFF, 0, 0]);
    assert_eq!(&res[12..16], &[0xFF, 0xFF, 0, 0]);
    assert!(res[16..32].iter().all(|&b| b == 0));
}

#[test]
fn manifest_resource_has_type_24_id_1_neutral_language_and_the_exact_bytes() {
    let res = build_res("<a/>", None).unwrap();
    assert_eq!(resources(&res), vec![(24, 1, b"<a/>".to_vec())]);
    assert_eq!(u16_at(&res, 32 + 22), LANGUAGE_NEUTRAL);
}

#[test]
fn ico_images_become_rt_icon_resources_with_ids_1_to_n_in_directory_order() {
    let res = build_res("m", Some(&ico(&[5, 8]))).unwrap();
    let r = resources(&res);
    assert_eq!((r[1].0, r[1].1, r[1].2.clone()), (3, 1, vec![1; 5]));
    assert_eq!((r[2].0, r[2].1, r[2].2.clone()), (3, 2, vec![2; 8]));
}

#[test]
fn group_icon_resource_has_id_32512_and_entries_with_the_rt_icon_ids() {
    let res = build_res("m", Some(&ico(&[5, 8]))).unwrap();
    let (kind, id, data) = resources(&res).pop().unwrap();
    assert_eq!((kind, id), (14, 32512));
    assert_eq!(data.len(), 6 + 2 * 14);
    assert_eq!((u16_at(&data, 2), u16_at(&data, 4)), (1, 2));
    assert_eq!(u32_at(&data, 6 + 8), 5);
    assert_eq!(u16_at(&data, 6 + 12), 1);
    assert_eq!(u32_at(&data, 6 + 14 + 8), 8);
    assert_eq!(u16_at(&data, 6 + 14 + 12), 2);
}

#[test]
fn every_resource_header_and_data_block_is_four_byte_aligned() {
    let res = build_res("odd", Some(&ico(&[5, 7]))).unwrap();
    assert_eq!(res.len() % 4, 0);
    resources(&res);
}

#[test]
fn no_icon_gives_only_the_empty_header_and_the_manifest() {
    assert_eq!(resources(&build_res("m", None).unwrap()).len(), 1);
}

#[test]
fn bytes_that_are_not_an_ico_are_rejected() {
    let mut bad = ico(&[4]);
    bad[0] = 1;
    assert!(matches!(parse_ico(&bad), Err(ResError::NotAnIco(_))));
    let mut bad = ico(&[4]);
    bad[2] = 2;
    assert!(matches!(parse_ico(&bad), Err(ResError::NotAnIco(_))));
    let mut bad = ico(&[4]);
    bad[4] = 0;
    assert!(matches!(parse_ico(&bad), Err(ResError::NotAnIco(_))));
    assert!(matches!(parse_ico(&[0, 0, 1]), Err(ResError::NotAnIco(_))));
}

#[test]
fn an_entry_pointing_outside_the_file_is_truncated() {
    let mut b = ico(&[4]);
    b.pop();
    assert_eq!(parse_ico(&b), Err(ResError::Truncated));
    let mut b = ico(&[4]);
    b[6 + 12..6 + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(parse_ico(&b), Err(ResError::Truncated));
    let mut b = ico(&[4]);
    b[4] = 3;
    assert_eq!(parse_ico(&b[..6 + 16]), Err(ResError::Truncated));
}

#[test]
fn release_version_is_type_16_id_1_en_us_without_changing_manifest() {
    let mut res = build_res("manifest", None).unwrap();
    append_version(&mut res, &[1, 2, 3, 4, 5]);
    assert_eq!(
        resources(&res),
        vec![(24, 1, b"manifest".to_vec()), (16, 1, vec![1, 2, 3, 4, 5])]
    );
    let header = 32 + 32 + 8;
    assert_eq!(u16_at(&res, header + 22), 0x0409);
}
