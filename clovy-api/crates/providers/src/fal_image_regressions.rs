use super::*;

#[test]
fn f5_inline_image_enforces_exact_decoded_cap() {
    // The two sizes occupy the same base64 length. Allocate one URI at a time.
    let cap = clovy_config::IMAGE_EDIT_SOURCE_MAX_BYTES;
    assert_eq!(cap, 52_428_800);
    assert_eq!(cap.div_ceil(3) * 4, (cap + 1).div_ceil(3) * 4);
    for size in [cap, cap + 1, cap + 2] {
        let mut bytes = vec![0; size];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let mut uri = String::from("data:image/jpeg;base64,");
        base64::engine::general_purpose::STANDARD.encode_string(&bytes, &mut uri);
        drop(bytes);
        let result = image_from_data_uri(&uri);
        if size == cap {
            let asset = result.unwrap();
            assert_eq!(asset.bytes.len(), cap);
            assert_eq!(asset.mime_type, "image/png");
        } else {
            assert!(
                matches!(result, Err(FalError::TooLarge)),
                "decoded cap + {} must be TooLarge",
                size - cap
            );
        }
    }
    for uri in [
        "data:image/png;base64,%%%",
        "data:image/png,AAAA",
        "data:image/png;base64,AAAA",
        "data:image/png;base64",
    ] {
        assert!(matches!(
            image_from_data_uri(uri),
            Err(FalError::MediaUnavailable)
        ));
    }
}
