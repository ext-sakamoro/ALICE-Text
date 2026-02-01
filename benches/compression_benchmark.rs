//! Compression benchmarks for ALICE-Text

use alice_text::{ALICEText, EncodingMode};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

fn generate_log_data(lines: usize) -> String {
    (0..lines)
        .map(|i| {
            format!(
                "2024-01-{:02} {:02}:{:02}:{:02} {} User {} logged in from 192.168.1.{}\n",
                (i % 28) + 1,
                i % 24,
                i % 60,
                i % 60,
                ["INFO", "WARN", "ERROR", "DEBUG"][i % 4],
                i % 1000,
                i % 256
            )
        })
        .collect()
}

fn compress_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("compress");

    // Small data (1KB)
    let small_data = generate_log_data(15);
    group.throughput(Throughput::Bytes(small_data.len() as u64));
    group.bench_function("1kb_pattern", |b| {
        b.iter(|| {
            let mut alice = ALICEText::new(EncodingMode::Pattern);
            alice.compress(black_box(&small_data)).unwrap()
        })
    });

    // Medium data (10KB)
    let medium_data = generate_log_data(150);
    group.throughput(Throughput::Bytes(medium_data.len() as u64));
    group.bench_function("10kb_pattern", |b| {
        b.iter(|| {
            let mut alice = ALICEText::new(EncodingMode::Pattern);
            alice.compress(black_box(&medium_data)).unwrap()
        })
    });

    // Large data (100KB)
    let large_data = generate_log_data(1500);
    group.throughput(Throughput::Bytes(large_data.len() as u64));
    group.bench_function("100kb_pattern", |b| {
        b.iter(|| {
            let mut alice = ALICEText::new(EncodingMode::Pattern);
            alice.compress(black_box(&large_data)).unwrap()
        })
    });

    group.finish();
}

fn decompress_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("decompress");

    // Prepare compressed data
    let data = generate_log_data(150);
    let mut alice = ALICEText::new(EncodingMode::Pattern);
    let compressed = alice.compress(&data).unwrap();

    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("10kb_pattern", |b| {
        b.iter(|| {
            let alice = ALICEText::default();
            alice.decompress(black_box(&compressed)).unwrap()
        })
    });

    group.finish();
}

fn roundtrip_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    let data = generate_log_data(150);
    group.throughput(Throughput::Bytes(data.len() as u64));

    group.bench_function("10kb_pattern", |b| {
        b.iter(|| {
            let mut alice = ALICEText::new(EncodingMode::Pattern);
            let compressed = alice.compress(black_box(&data)).unwrap();
            alice.decompress(&compressed).unwrap()
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    compress_benchmark,
    decompress_benchmark,
    roundtrip_benchmark
);
criterion_main!(benches);
