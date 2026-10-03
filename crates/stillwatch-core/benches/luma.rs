//! Luma conversion, downscale, and block means on synthetic 4K frames.
//!
//! Expected numbers from `cargo bench -p stillwatch-core --bench luma` on
//! the target machine (Ryzen 9 9950X3D, Rust 1.99, release profile, measured
//! with other builds running):
//!
//! | Bench | Time |
//! | -- | -- |
//! | `downscale/rgb32_3840x2160_to_480` | 3.6 ms |
//! | `downscale/a2bgr30_3840x2160_to_480` | 5.1 ms |
//! | `downscale/rgba16fpx4_3840x2160_to_480` | 9.3 ms |
//! | `block_means/480x270_16x16` | 9.6 µs |
//!
//! The budget is 50 ms for a 4K conversion plus downscale and 1 ms for
//! detection over the grid.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use stillwatch_core::luma::{LumaGrid, PixelFormat, RawFrame, block_means, downscale};

const WIDTH: u32 = 3840;
const HEIGHT: u32 = 2160;

/// A deterministic noise frame, so no format decodes to a constant.
fn synthetic(format: PixelFormat) -> Vec<u8> {
    let len = WIDTH as usize * HEIGHT as usize * format.bytes_per_pixel();
    let mut state = 0x9e37_79b9_u32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect()
}

/// Rewrites every half-float channel to a value in [0, 1.5), so the bench
/// measures real decodes rather than NaN and infinity shortcuts.
fn tame_halves(data: &mut [u8]) {
    for half in data.as_chunks_mut::<2>().0 {
        let mantissa = u16::from_le_bytes(*half) & 0x03ff;
        *half = (0x3800 | mantissa).to_ne_bytes();
    }
}

fn bench_downscale(c: &mut Criterion) {
    let mut group = c.benchmark_group("downscale");
    let cases = [
        ("rgb32", PixelFormat::Rgb32),
        ("a2bgr30", PixelFormat::A2Bgr30Premultiplied),
        ("rgba16fpx4", PixelFormat::Rgba16Fpx4),
    ];
    for (name, format) in cases {
        let mut data = synthetic(format);
        if format == PixelFormat::Rgba16Fpx4 {
            tame_halves(&mut data);
        }
        let frame = RawFrame {
            format,
            width: WIDTH,
            height: HEIGHT,
            stride: WIDTH as usize * format.bytes_per_pixel(),
            data: &data,
        };
        group.bench_function(format!("{name}_{WIDTH}x{HEIGHT}_to_480"), |b| {
            b.iter(|| downscale(black_box(&frame), black_box(480)));
        });
    }
    group.finish();
}

fn bench_block_means(c: &mut Criterion) {
    let noise = synthetic(PixelFormat::Rgb32);
    let Ok(grid) = LumaGrid::new(480, 270, noise[..480 * 270].to_vec()) else {
        return;
    };
    c.bench_function("block_means/480x270_16x16", |b| {
        b.iter(|| block_means(black_box(&grid), 16, 16));
    });
}

criterion_group!(benches, bench_downscale, bench_block_means);
criterion_main!(benches);
