use zbus::zvariant::{OwnedValue, Value};

use super::*;

fn owned(value: Value<'_>) -> OwnedValue {
    value.try_into_owned().unwrap()
}

/// The reply `KWin` 6.7.5 sends for a 3840x2160 output.
fn kwin_reply() -> HashMap<String, OwnedValue> {
    HashMap::from([
        ("type".to_owned(), owned(Value::from("raw"))),
        ("format".to_owned(), owned(Value::from(6_u32))),
        ("width".to_owned(), owned(Value::from(3840_u32))),
        ("height".to_owned(), owned(Value::from(2160_u32))),
        ("stride".to_owned(), owned(Value::from(15360_u32))),
        ("scale".to_owned(), owned(Value::from(1.0_f64))),
        ("screen".to_owned(), owned(Value::from("HDMI-A-1"))),
    ])
}

#[test]
fn parses_a_kwin_reply() {
    let meta = FrameMeta::parse(&kwin_reply()).unwrap();
    assert_eq!(
        meta,
        FrameMeta {
            format: PixelFormat::Argb32Premultiplied,
            width: 3840,
            height: 2160,
            stride: 15360,
            scale: Some(1.0),
            screen: Some("HDMI-A-1".into()),
        }
    );
    assert_eq!(meta.byte_len().unwrap(), 15360 * 2160);
}

#[test]
fn type_scale_and_screen_are_optional() {
    let mut reply = kwin_reply();
    for key in ["type", "scale", "screen"] {
        reply.remove(key);
    }
    let meta = FrameMeta::parse(&reply).unwrap();
    assert_eq!((meta.scale, meta.screen), (None, None));
}

#[test]
fn missing_dimensions_are_protocol_errors() {
    for key in ["format", "width", "height", "stride"] {
        let mut reply = kwin_reply();
        reply.remove(key);
        let err = FrameMeta::parse(&reply).unwrap_err();
        assert_eq!(
            err,
            BackendError::Protocol(format!("ScreenShot2 reply has no {key:?}"))
        );
    }
}

#[test]
fn mistyped_values_are_protocol_errors() {
    let mut reply = kwin_reply();
    reply.insert("width".into(), owned(Value::from(3840_i32)));
    let err = FrameMeta::parse(&reply).unwrap_err();
    assert!(
        matches!(&err, BackendError::Protocol(m) if m.starts_with("ScreenShot2 reply \"width\" has the wrong type (i)")),
        "{err}"
    );
}

#[test]
fn non_raw_images_are_unsupported() {
    let mut reply = kwin_reply();
    reply.insert("type".into(), owned(Value::from("png")));
    assert_eq!(
        FrameMeta::parse(&reply),
        Err(BackendError::Unsupported(
            "ScreenShot2 image type \"png\"".into()
        ))
    );
}

#[test]
fn unknown_formats_are_unsupported() {
    let mut reply = kwin_reply();
    reply.insert("format".into(), owned(Value::from(13_u32)));
    assert_eq!(
        FrameMeta::parse(&reply),
        Err(BackendError::Unsupported(
            "unsupported QImage format 13".into()
        ))
    );
}

#[test]
fn oversized_frames_are_refused() {
    let mut meta = FrameMeta::parse(&kwin_reply()).unwrap();
    meta.stride = u32::MAX;
    meta.height = u32::MAX;
    let err = meta.byte_len().unwrap_err();
    assert!(
        matches!(&err, BackendError::Protocol(m) if m.contains("larger than")),
        "{err}"
    );
}

#[test]
fn frame_borrows_the_buffer_with_the_reply_layout() {
    let meta = FrameMeta {
        format: PixelFormat::Rgb32,
        width: 2,
        height: 1,
        stride: 8,
        scale: None,
        screen: None,
    };
    let data = [0_u8; 8];
    let frame = meta.frame(&data);
    assert_eq!(
        (frame.format, frame.width, frame.height, frame.stride),
        (PixelFormat::Rgb32, 2, 1, 8)
    );
    assert_eq!(frame.data.len(), 8);
}
