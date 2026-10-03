//! Detection over the default 16x16 grid. Budget: under 1 ms per capture.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use stillwatch_core::backend::MediaPlayer;
use stillwatch_core::config::Config;
use stillwatch_core::detector::{BlockDetector, BlockMeans};

fn means(seed: u8) -> Vec<f32> {
    (0..=u8::MAX)
        .map(|block| f32::from(block.wrapping_mul(31).wrapping_add(seed)))
        .collect()
}

fn detection(c: &mut Criterion) {
    let config = Config::default();
    let frames = [means(0), means(7)];
    let playing = vec![MediaPlayer::named("spotify")];
    let mut detector = BlockDetector::new(&config);
    let mut tick = 0usize;
    c.bench_function("detect_16x16", |b| {
        b.iter(|| {
            tick = tick.wrapping_add(1);
            let frame = [BlockMeans {
                output: "HDMI-A-1",
                means: &frames[tick % 2],
            }];
            black_box(detector.observe_means(black_box(&frame), &playing))
        });
    });
}

criterion_group!(benches, detection);
criterion_main!(benches);
