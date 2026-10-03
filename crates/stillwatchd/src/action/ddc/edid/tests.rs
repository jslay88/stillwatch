use super::*;
use crate::action::ddc::fixtures::{PG48UQ, dell, pg48uq_unit};

#[test]
fn parses_the_real_pg48uq_block() {
    let identity = parse(&PG48UQ).unwrap();
    assert_eq!(
        identity,
        EdidIdentity {
            manufacturer: "AUS".into(),
            product: 0x48E0,
            serial: 0xFFFF_FFFF,
            serial_text: None,
            name: Some("PG48UQ".into()),
        }
    );
    assert_eq!(
        identity.to_string(),
        "AUS 0x48e0 \"PG48UQ\" serial 0xffffffff"
    );
}

#[test]
fn extension_blocks_are_ignored() {
    let mut full = PG48UQ.to_vec();
    full.extend_from_slice(&[0xAB; 256]);
    assert_eq!(parse(&full).unwrap(), parse(&PG48UQ).unwrap());
}

#[test]
fn reads_the_serial_descriptor() {
    let identity = parse(&pg48uq_unit(7, "R9LMQS123456")).unwrap();
    assert_eq!(identity.serial, 7);
    assert_eq!(identity.serial_text.as_deref(), Some("R9LMQS123456"));
    assert_eq!(
        identity.to_string(),
        "AUS 0x48e0 \"PG48UQ\" serial 0x00000007 \"R9LMQS123456\""
    );
    let full = parse(&pg48uq_unit(7, "ABCDEFGHIJKLM")).unwrap();
    assert_eq!(full.serial_text.as_deref(), Some("ABCDEFGHIJKLM"));
}

#[test]
fn decodes_other_manufacturers() {
    let identity = parse(&dell()).unwrap();
    assert_eq!(identity.manufacturer, "DEL");
    assert_eq!(identity.product, 0xA0B1);
    assert_eq!(identity.serial, 42);
}

#[test]
fn same_unit_compares_every_identity_field_but_the_name() {
    let base = parse(&PG48UQ).unwrap();
    let mut renamed = base.clone();
    renamed.name = Some("other".into());
    assert!(base.same_unit(&renamed));
    assert!(!base.same_unit(&parse(&dell()).unwrap()));
    assert!(!base.same_unit(&parse(&pg48uq_unit(0xFFFF_FFFF, "X")).unwrap()));
    assert!(!base.same_unit(&parse(&pg48uq_unit(1, "")).unwrap()));
}

#[test]
fn rejects_short_and_headerless_blocks() {
    assert_eq!(parse(&PG48UQ[..127]), Err(EdidError::TooShort(127)));
    assert_eq!(parse(&[]), Err(EdidError::TooShort(0)));
    let mut headerless = PG48UQ;
    headerless[0] = 0x01;
    assert_eq!(parse(&headerless), Err(EdidError::BadHeader));
}

#[test]
fn rejects_manufacturer_ids_that_arent_letters() {
    for packed in [0x8000u16 | 0x06b3, 0x0000, 0x7FFF] {
        let mut edid = PG48UQ;
        edid[8..10].copy_from_slice(&packed.to_be_bytes());
        assert_eq!(parse(&edid), Err(EdidError::BadManufacturer(packed)));
    }
}

#[test]
fn errors_describe_the_problem() {
    assert_eq!(
        EdidError::TooShort(3).to_string(),
        "EDID is 3 bytes, shorter than the 128-byte base block"
    );
    assert_eq!(EdidError::BadHeader.to_string(), "EDID header is missing");
    assert_eq!(
        EdidError::BadManufacturer(0).to_string(),
        "EDID manufacturer ID 0x0000 isn't three letters"
    );
}

#[test]
fn base_block_needs_128_bytes() {
    assert_eq!(base_block(&PG48UQ).map(<[u8]>::len), Some(128));
    assert_eq!(base_block(&PG48UQ[..10]), None);
}
