//! Compression test example

use alice_text::{ALICEText, EncodingMode, TunedCompressor, CompressionMode};

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

fn main() {
    println!("=== ALICE-Text Compression Comparison ===\n");

    // Test different data sizes
    let sizes = [
        ("1KB", 15),
        ("10KB", 150),
        ("100KB", 1500),
    ];

    println!("--- Original ALICEText (LZMA) ---\n");

    for (name, lines) in sizes {
        let data = generate_log_data(lines);
        let original_size = data.len();

        let mut alice = ALICEText::new(EncodingMode::Pattern);

        let start = std::time::Instant::now();
        let compressed = alice.compress(&data).unwrap();
        let compress_time = start.elapsed();

        let compressed_size = compressed.len();

        let start = std::time::Instant::now();
        let decompressed = alice.decompress(&compressed).unwrap();
        let decompress_time = start.elapsed();

        assert_eq!(data, decompressed);

        let ratio = compressed_size as f64 / original_size as f64 * 100.0;
        let savings = 100.0 - ratio;

        println!("{} Log Data:", name);
        println!("  Original:   {} bytes", original_size);
        println!("  Compressed: {} bytes", compressed_size);
        println!("  Ratio:      {:.1}%", ratio);
        println!("  Savings:    {:.1}%", savings);
        println!("  Compress:   {:?}", compress_time);
        println!("  Decompress: {:?}", decompress_time);
        println!();
    }

    println!("--- TunedCompressor (Zstd + Bincode + Columnar) ---\n");

    for (name, lines) in sizes {
        let data = generate_log_data(lines);
        let original_size = data.len();

        let mut tuned = TunedCompressor::new(CompressionMode::Balanced);

        let start = std::time::Instant::now();
        let compressed = tuned.compress(&data).unwrap();
        let compress_time = start.elapsed();

        let compressed_size = compressed.len();

        let start = std::time::Instant::now();
        let decompressed = tuned.decompress(&compressed).unwrap();
        let decompress_time = start.elapsed();

        assert_eq!(data, decompressed);

        let ratio = compressed_size as f64 / original_size as f64 * 100.0;
        let savings = 100.0 - ratio;

        println!("{} Log Data:", name);
        println!("  Original:   {} bytes", original_size);
        println!("  Compressed: {} bytes", compressed_size);
        println!("  Ratio:      {:.1}%", ratio);
        println!("  Savings:    {:.1}%", savings);
        println!("  Compress:   {:?}", compress_time);
        println!("  Decompress: {:?}", decompress_time);
        println!();
    }

    // Test entropy estimation
    let data = generate_log_data(150);
    let alice = ALICEText::default();
    let estimate = alice.estimate_compression(&data);

    println!("=== Entropy Estimation (10KB) ===");
    println!("  Shannon Entropy: {:.2} bits/byte", estimate.shannon_entropy);
    println!("  Pattern Coverage: {:.1}%", estimate.pattern_coverage * 100.0);
    println!("  Repetition Score: {:.2}", estimate.repetition_score);
    println!("  Estimated Ratio: {:.1}%", estimate.estimated_ratio * 100.0);
    println!("  Quality: {}", estimate.quality());
}
