use pipewire::spa::buffer::DataType;
use pipewire::spa::param::video::VideoFormat;
use stillwatch_core::backend::BackendError;
use stillwatch_core::luma::PixelFormat;

use super::{Plane, copy_mapped, grid_from_plane};

const WIDTH: u32 = 4;
const HEIGHT: u32 = 2;

/// Left half white, right half black, SPA `RGBx` (bytes R, G, B, X).
fn rgbx() -> Vec<u8> {
    let mut data = Vec::new();
    for _ in 0..HEIGHT {
        for x in 0..WIDTH {
            if x < WIDTH / 2 {
                data.extend_from_slice(&[255, 255, 255, 0]);
            } else {
                data.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    data
}

#[test]
fn a_mapped_frame_becomes_a_luma_grid_and_the_bytes_are_not_kept() {
    let bytes = rgbx();
    let grid = grid_from_plane(
        &Plane {
            spa_format: VideoFormat::RGBx.0,
            width: WIDTH,
            height: HEIGHT,
            stride: 0,
            data: &bytes,
        },
        2,
    )
    .unwrap();
    assert_eq!((grid.width(), grid.height()), (2, 1));
    assert_eq!(grid.data(), &[255, 0]);
    drop(bytes);
}

#[test]
fn spa_codes_match_the_headers_pipewire_was_built_with() {
    let cases = [
        (VideoFormat::RGBx, PixelFormat::Rgbx8888),
        (VideoFormat::BGRx, PixelFormat::Bgrx8888),
        (VideoFormat::xRGB, PixelFormat::Xrgb8888),
        (VideoFormat::RGBA, PixelFormat::Rgba8888),
        (VideoFormat::BGRA, PixelFormat::Bgra8888),
        (VideoFormat::ARGB, PixelFormat::Argb8888),
        (VideoFormat::RGBA_F16, PixelFormat::Rgba16Fpx4),
        (VideoFormat::RGBA_F32, PixelFormat::Rgba32Fpx4),
    ];
    for (spa, pixel) in cases {
        assert_eq!(PixelFormat::from_spa(spa.0).unwrap(), pixel);
    }
    assert!(PixelFormat::from_spa(VideoFormat::xBGR.0).is_err());
}

#[test]
fn an_unmapped_dma_buf_is_an_error() {
    let err = copy_mapped(DataType::DmaBuf, None, 0, 16).unwrap_err();
    assert!(matches!(err, BackendError::Unsupported(_)));
    assert!(err.to_string().contains("DmaBuf"));
}

#[test]
fn a_short_mapping_is_a_protocol_error() {
    let err = copy_mapped(DataType::MemFd, Some(&[1, 2, 3]), 0, 8).unwrap_err();
    assert!(matches!(err, BackendError::Protocol(_)));
}
