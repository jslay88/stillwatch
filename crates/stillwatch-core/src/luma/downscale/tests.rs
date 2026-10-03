use proptest::prelude::*;

use super::*;
use crate::backend::BackendError;
use crate::luma::PixelFormat;

const WHITE: [u8; 3] = [255, 255, 255];
const BLACK: [u8; 3] = [0, 0, 0];

/// Rec. 709 luma of 8-bit channels, rounded: what every format should give
/// for a pixel at these levels.
const KNOWN: [([u8; 3], u8); 6] = [
    (WHITE, 255),
    (BLACK, 0),
    ([128, 128, 128], 128),
    ([255, 0, 0], 54),
    ([0, 255, 0], 182),
    ([0, 0, 255], 18),
];

fn f32_to_f16(value: f32) -> u16 {
    if value == 0.0 {
        return 0;
    }
    let bits = value.to_bits();
    let sign = (bits >> 16) & 0x8000;
    let exponent = ((bits >> 23) & 0xff) - 112;
    let mantissa = (bits >> 13) & 0x3ff;
    u16::try_from(sign | (exponent << 10) | mantissa).unwrap()
}

/// One pixel at 8-bit `levels`, widened to the format's depth.
fn encode(format: PixelFormat, levels: [u8; 3]) -> Vec<u8> {
    let [r, g, b] = levels.map(u32::from);
    let ten = |v: u32| (v * 1023 + 127) / 255;
    let half = |v: u32| f32_to_f16(f32::from(u8::try_from(v).unwrap()) / 255.0);
    match format.layout() {
        Layout::Argb32Word => (0xff00_0000 | r << 16 | g << 8 | b).to_ne_bytes().to_vec(),
        Layout::BytesRgbx => vec![levels[0], levels[1], levels[2], 0xff],
        Layout::BytesBgrx => vec![levels[2], levels[1], levels[0], 0xff],
        Layout::BytesXrgb => vec![0xff, levels[0], levels[1], levels[2]],
        Layout::Bgr30Word => (3 << 30 | ten(b) << 20 | ten(g) << 10 | ten(r))
            .to_ne_bytes()
            .to_vec(),
        Layout::Rgb30Word => (3 << 30 | ten(r) << 20 | ten(g) << 10 | ten(b))
            .to_ne_bytes()
            .to_vec(),
        Layout::Rgba16 => [r * 257, g * 257, b * 257, 0xffff]
            .into_iter()
            .flat_map(|v| u16::try_from(v).unwrap().to_ne_bytes())
            .collect(),
        Layout::Rgba16F => [half(r), half(g), half(b), 0x3c00]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect(),
        Layout::Rgba32F => [r, g, b, 255]
            .into_iter()
            .flat_map(|v| (f32::from(u8::try_from(v).unwrap()) / 255.0).to_ne_bytes())
            .collect(),
    }
}

/// A tightly packed frame whose pixel `(x, y)` is at `pixel(x, y)`.
fn image(
    format: PixelFormat,
    width: u32,
    height: u32,
    pixel: impl Fn(u32, u32) -> [u8; 3],
) -> Vec<u8> {
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| encode(format, pixel(x, y)))
        .collect()
}

fn run(
    format: PixelFormat,
    width: u32,
    height: u32,
    data: &[u8],
    downscale_width: u32,
) -> Result<LumaGrid, FrameError> {
    let stride = width as usize * format.bytes_per_pixel();
    let frame = RawFrame {
        format,
        width,
        height,
        stride,
        data,
    };
    downscale(&frame, downscale_width)
}

#[test]
fn every_format_decodes_known_values() {
    for format in PixelFormat::ALL {
        for (levels, expected) in KNOWN {
            let data = encode(format, levels);
            let grid = run(format, 1, 1, &data, 480).unwrap();
            assert_eq!(grid.data(), &[expected], "{format:?} {levels:?}");
        }
    }
}

#[test]
fn alpha_is_ignored() {
    let word = 0x00ff_ffff_u32.to_ne_bytes();
    assert_eq!(
        run(PixelFormat::Argb32, 1, 1, &word, 1).unwrap().data(),
        &[255]
    );
    let bytes = [0, 255, 255, 255];
    assert_eq!(
        run(PixelFormat::Argb8888, 1, 1, &bytes, 1).unwrap().data(),
        &[255]
    );
}

#[test]
fn hdr_floats_clamp_to_white_and_black() {
    let halves = |r: u16, g: u16, b: u16| -> Vec<u8> {
        [r, g, b, 0x3c00]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect()
    };
    let over = halves(0x4000, 0x4000, 0x4000);
    let under = halves(0xbc00, 0xbc00, 0xbc00);
    assert_eq!(
        run(PixelFormat::Rgba16Fpx4, 1, 1, &over, 1).unwrap().data(),
        &[255]
    );
    assert_eq!(
        run(PixelFormat::Rgbx16Fpx4, 1, 1, &under, 1)
            .unwrap()
            .data(),
        &[0]
    );
    let floats: Vec<u8> = [4.0_f32, f32::NAN, 4.0, 1.0]
        .into_iter()
        .flat_map(f32::to_ne_bytes)
        .collect();
    let grid = run(PixelFormat::Rgba32Fpx4Premultiplied, 1, 1, &floats, 1).unwrap();
    assert_eq!(grid.data(), &[73]);
}

#[test]
fn stride_padding_is_skipped() {
    let format = PixelFormat::Bgrx8888;
    let row = image(format, 3, 1, |_, _| BLACK);
    let padding = [0xff_u8; 5];
    let data = [row.as_slice(), &padding, &row].concat();
    let frame = RawFrame {
        format,
        width: 3,
        height: 2,
        stride: row.len() + padding.len(),
        data: &data,
    };
    assert_eq!(downscale(&frame, 8).unwrap().data(), &[0; 6]);
    let padded = [data.as_slice(), &padding].concat();
    let padded_last_row = RawFrame {
        data: &padded,
        ..frame
    };
    assert_eq!(downscale(&padded_last_row, 8).unwrap().data(), &[0; 6]);
}

#[test]
fn odd_sizes_average_every_pixel() {
    let format = PixelFormat::Rgb32;
    let data = image(format, 5, 3, |x, _| if x < 2 { WHITE } else { BLACK });
    let grid = run(format, 5, 3, &data, 2).unwrap();
    assert_eq!((grid.width(), grid.height()), (2, 1));
    assert_eq!(grid.data(), &[255, 0]);

    let dot = image(
        format,
        8,
        8,
        |x, y| if (x, y) == (5, 6) { WHITE } else { BLACK },
    );
    assert_eq!(run(format, 8, 8, &dot, 1).unwrap().data(), &[4]);
}

#[test]
fn narrow_frames_are_not_upscaled() {
    let format = PixelFormat::Rgbx8888;
    let data = image(format, 4, 2, |x, y| if x == y { WHITE } else { BLACK });
    let grid = run(format, 4, 2, &data, 480).unwrap();
    assert_eq!((grid.width(), grid.height()), (4, 2));
    assert_eq!(grid.data(), &[255, 0, 0, 0, 0, 255, 0, 0]);
}

#[test]
fn aspect_ratio_is_preserved() {
    let cases = [
        ((1920, 1080, 480), (480, 270)),
        ((16, 9, 8), (8, 5)),
        ((1000, 1, 480), (480, 1)),
        ((1, 1000, 480), (1, 1000)),
        ((2560, 1440, 7), (7, 4)),
    ];
    for ((width, height, downscale_width), expected) in cases {
        let format = PixelFormat::Argb32Premultiplied;
        let data = vec![0; width as usize * height as usize * 4];
        let grid = run(format, width, height, &data, downscale_width).unwrap();
        assert_eq!((grid.width(), grid.height()), expected, "{width}x{height}");
    }
}

#[test]
fn invalid_frames_error() {
    let format = PixelFormat::Rgb32;
    let data = [0_u8; 32];
    let frame = |width, height, stride, data| RawFrame {
        format,
        width,
        height,
        stride,
        data,
    };
    let cases = [
        (
            frame(0, 2, 16, &data[..]),
            4,
            FrameError::Empty {
                width: 0,
                height: 2,
            },
        ),
        (
            frame(2, 0, 16, &data),
            4,
            FrameError::Empty {
                width: 2,
                height: 0,
            },
        ),
        (frame(2, 2, 8, &data), 0, FrameError::ZeroDownscaleWidth),
        (
            frame(4, 2, 12, &data),
            4,
            FrameError::StrideTooSmall {
                stride: 12,
                row_bytes: 16,
            },
        ),
        (
            frame(4, 2, 20, &data),
            4,
            FrameError::BufferTooShort {
                expected: 36,
                actual: 32,
            },
        ),
        (
            frame(4, 3, usize::MAX, &data),
            4,
            FrameError::TooLarge {
                width: 4,
                height: 3,
            },
        ),
    ];
    for (frame, downscale_width, expected) in cases {
        assert_eq!(downscale(&frame, downscale_width), Err(expected));
    }
}

#[test]
fn errors_read_well_and_map_to_backend_errors() {
    let err = FrameError::BufferTooShort {
        expected: 36,
        actual: 32,
    };
    assert_eq!(
        err.to_string(),
        "frame buffer has 32 bytes, expected at least 36"
    );
    assert_eq!(
        BackendError::from(err),
        BackendError::Protocol("frame buffer has 32 bytes, expected at least 36".into())
    );
    let messages = [
        (
            FrameError::Empty {
                width: 0,
                height: 2,
            },
            "frame is empty (0x2)",
        ),
        (
            FrameError::ZeroDownscaleWidth,
            "downscale width must be at least 1",
        ),
        (
            FrameError::StrideTooSmall {
                stride: 12,
                row_bytes: 16,
            },
            "stride of 12 bytes is shorter than a 16-byte row",
        ),
        (
            FrameError::TooLarge {
                width: 4,
                height: 3,
            },
            "frame dimensions 4x3 overflow",
        ),
    ];
    for (err, message) in messages {
        assert_eq!(err.to_string(), message);
    }
    let unsupported = PixelFormat::from_qimage(7).unwrap_err();
    assert_eq!(
        BackendError::from(unsupported),
        BackendError::Unsupported("unsupported QImage format 7".into())
    );
}

#[test]
fn debug_output_omits_pixels() {
    let data = [0xab_u8; 8];
    let frame = RawFrame {
        format: PixelFormat::Rgb30,
        width: 2,
        height: 1,
        stride: 8,
        data: &data,
    };
    assert_eq!(
        format!("{frame:?}"),
        "RawFrame { format: Rgb30, width: 2, height: 1, stride: 8, data_len: 8 }"
    );
}

fn any_format() -> impl Strategy<Value = PixelFormat> {
    prop::sample::select(PixelFormat::ALL.to_vec())
}

proptest! {
    #[test]
    fn uniform_frames_give_uniform_grids(
        format in any_format(),
        width in 1_u32..48,
        height in 1_u32..48,
        downscale_width in 1_u32..64,
        level in any::<u8>(),
    ) {
        let data = image(format, width, height, |_, _| [level; 3]);
        let grid = run(format, width, height, &data, downscale_width).unwrap();
        prop_assert_eq!(grid.width(), downscale_width.min(width));
        let expected = run(format, 1, 1, &encode(format, [level; 3]), 1).unwrap().data()[0];
        prop_assert!(grid.data().iter().all(|&luma| luma == expected));
    }

    #[test]
    fn cells_stay_between_the_darkest_and_brightest_pixel(
        format in any_format(),
        width in 1_u32..40,
        height in 1_u32..40,
        downscale_width in 1_u32..40,
        seed in any::<u32>(),
    ) {
        let pixel = |x: u32, y: u32| {
            let n = (x * 7919 + y * 104_729) ^ seed;
            [n, n >> 8, n >> 16].map(|v| u8::try_from(v & 0xff).unwrap())
        };
        let data = image(format, width, height, pixel);
        let full = run(format, width, height, &data, width).unwrap();
        let grid = run(format, width, height, &data, downscale_width).unwrap();
        let min = *full.data().iter().min().unwrap();
        let max = *full.data().iter().max().unwrap();
        prop_assert!(grid.data().iter().all(|&luma| (min..=max).contains(&luma)));
    }
}
