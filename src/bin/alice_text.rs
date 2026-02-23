//! ALICE-Text CLI
//!
//! Command-line interface for ALICE-Text compression.

use alice_text::{
    compress_v3, ALICEText, CompressionLevel, CompressionMode, EntropyEstimator, FormatV3Metadata,
    Op, QueryEngine, TunedCompressor,
};
use clap::{Parser, Subcommand};
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "alice-text")]
#[command(author = "Moroya Sakamoto")]
#[command(version = "1.0.0")]
#[command(about = "Exception-based text compression - Send only surprises, not predictions")]
#[command(long_about = r#"
ALICE-Text: Exception-based Text Compression

Principle:
  Input Text → Prediction Model P(next|context)
    → Prediction Success → Information = 0 → Don't send
    → Prediction Failure → Exception Token → Send

Only surprises are transmitted, not predictions.
"#)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compress a file
    Compress {
        /// Input file (use - for stdin)
        input: PathBuf,

        /// Output file (default: input.atxt)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Compression level: fast, balanced, best
        #[arg(short, long, default_value = "balanced")]
        level: String,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Decompress a file
    Decompress {
        /// Input file (.atxt)
        input: PathBuf,

        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Show file information
    Info {
        /// Input file (.atxt)
        input: PathBuf,
    },

    /// Estimate compression for a file
    Estimate {
        /// Input file
        input: PathBuf,

        /// Show detailed output
        #[arg(short, long)]
        detailed: bool,
    },

    /// Verify a compressed file
    Verify {
        /// Input file (.atxt)
        input: PathBuf,
    },

    /// Query compressed file (v3 format only)
    Query {
        /// Input file (.atxt, v3 format)
        input: PathBuf,

        /// Show column list only
        #[arg(long)]
        columns: bool,

        /// Show file statistics only
        #[arg(long)]
        stats: bool,

        /// Select specific columns (comma-separated)
        #[arg(short, long)]
        select: Option<String>,

        /// Filter condition: column=value
        #[arg(short = 'w', long = "where")]
        filter: Option<String>,

        /// Output format: table, csv, json
        #[arg(short, long, default_value = "table")]
        format: String,

        /// Limit output rows
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// Compress file using v3 format (columnar, queryable)
    CompressV3 {
        /// Input file
        input: PathBuf,

        /// Output file (default: input.atxt)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Compression level: fast, balanced, best
        #[arg(short, long, default_value = "balanced")]
        level: String,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compress {
            input,
            output,
            level,
            verbose,
        } => {
            compress_file(&input, output, &level, verbose)?;
        }
        Commands::Decompress { input, output } => {
            decompress_file(&input, output)?;
        }
        Commands::Info { input } => {
            show_info(&input)?;
        }
        Commands::Estimate { input, detailed } => {
            estimate_compression(&input, detailed)?;
        }
        Commands::Verify { input } => {
            verify_file(&input)?;
        }
        Commands::Query {
            input,
            columns,
            stats,
            select,
            filter,
            format,
            limit,
        } => {
            query_file(&input, columns, stats, select, filter, &format, limit)?;
        }
        Commands::CompressV3 {
            input,
            output,
            level,
            verbose,
        } => {
            compress_file_v3(&input, output, &level, verbose)?;
        }
    }

    Ok(())
}

fn compress_file(
    input: &PathBuf,
    output: Option<PathBuf>,
    level: &str,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read input
    let text = if input.to_string_lossy() == "-" {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else {
        fs::read_to_string(input)?
    };

    let original_size = text.len();

    // Parse compression level
    let compression_mode = match level.to_lowercase().as_str() {
        "fast" => CompressionMode::Fast,
        "balanced" => CompressionMode::Balanced,
        "best" => CompressionMode::Best,
        _ => {
            eprintln!("Unknown level: {}. Using balanced.", level);
            CompressionMode::Balanced
        }
    };

    // Compress using TunedCompressor v2
    let start = Instant::now();
    let mut compressor = TunedCompressor::new(compression_mode);
    let compressed = compressor.compress(&text)?;
    let elapsed = start.elapsed();

    let compressed_size = compressed.len();

    // Write output
    let output_path = output.unwrap_or_else(|| {
        let mut p = input.clone();
        p.set_extension("atxt");
        p
    });

    fs::write(&output_path, &compressed)?;

    // Report
    let ratio = compressed_size as f64 / original_size as f64 * 100.0;
    let savings = 100.0 - ratio;

    if verbose {
        println!("ALICE-Text Compression (v2)");
        println!("===========================");
        println!("Input:      {}", input.display());
        println!("Output:     {}", output_path.display());
        println!("Level:      {:?}", compression_mode);
        println!();
        println!("Original:   {} bytes", original_size);
        println!("Compressed: {} bytes", compressed_size);
        println!("Ratio:      {:.1}%", ratio);
        println!("Savings:    {:.1}%", savings);
        println!("Time:       {:.2}ms", elapsed.as_secs_f64() * 1000.0);

        if let Some(stats) = compressor.last_stats() {
            println!();
            println!("Statistics:");
            println!("  Patterns:   {}", stats.pattern_count);
            println!("  Skeleton:   {} tokens", stats.skeleton_size);
        }
    } else {
        println!(
            "{} -> {} ({:.1}% ratio, {:.1}% saved)",
            input.display(),
            output_path.display(),
            ratio,
            savings
        );
    }

    Ok(())
}

fn decompress_file(
    input: &PathBuf,
    output: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read compressed data
    let compressed = fs::read(input)?;

    // Decompress
    let alice = ALICEText::default();
    let text = alice.decompress(&compressed)?;

    // Write output
    if let Some(output_path) = output {
        fs::write(&output_path, &text)?;
        println!("Decompressed to: {}", output_path.display());
    } else {
        io::stdout().write_all(text.as_bytes())?;
    }

    Ok(())
}

fn show_info(input: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let compressed = fs::read(input)?;

    // Read header manually for info
    if compressed.len() < 34 {
        eprintln!("Error: File too small");
        return Ok(());
    }

    // Check magic
    if &compressed[0..8] != b"ALICETXT" {
        eprintln!("Error: Invalid ALICE-Text file (bad magic)");
        return Ok(());
    }

    let version = (compressed[8], compressed[9]);

    println!("ALICE-Text File Information");
    println!("===========================");
    println!("File:            {}", input.display());
    println!("Compressed Size: {} bytes", compressed.len());
    println!("Version:         {}.{}", version.0, version.1);

    if version.0 >= 2 {
        // v2 format (TunedCompressor)
        let original_length = u64::from_le_bytes(compressed[10..18].try_into().unwrap_or([0u8; 8]));
        let mode = match compressed[18] {
            0 => "Fast",
            1 => "Balanced",
            2 => "Best",
            _ => "Unknown",
        };
        let pattern_count = u32::from_le_bytes(compressed[22..26].try_into().unwrap_or([0u8; 4]));
        let skeleton_length = u32::from_le_bytes(compressed[26..30].try_into().unwrap_or([0u8; 4]));

        println!("Format:          Tuned (Zstd + Columnar)");
        println!("Compression:     {}", mode);
        println!("Original Size:   {} bytes", original_length);
        println!("Pattern Count:   {}", pattern_count);
        println!("Skeleton Tokens: {}", skeleton_length);

        let ratio = compressed.len() as f64 / original_length as f64 * 100.0;
        println!("Ratio:           {:.1}%", ratio);

        // Verify decompression
        let compressor = TunedCompressor::default();
        match compressor.decompress(&compressed) {
            Ok(_) => println!("Status:          Valid (decompression OK)"),
            Err(e) => println!("Status:          Invalid ({})", e),
        }
    } else {
        // v1 format (legacy)
        let mode = match compressed[10] {
            0 => "Pattern",
            1 => "N-gram",
            _ => "Unknown",
        };
        let original_length = u32::from_le_bytes(compressed[14..18].try_into().unwrap_or([0u8; 4]));
        let token_count = u32::from_le_bytes(compressed[18..22].try_into().unwrap_or([0u8; 4]));
        let exception_count = u32::from_le_bytes(compressed[22..26].try_into().unwrap_or([0u8; 4]));

        println!("Format:          Legacy (LZMA)");
        println!("Mode:            {}", mode);
        println!("Original Size:   {} bytes", original_length);
        println!("Token Count:     {}", token_count);
        println!("Exception Count: {}", exception_count);

        let ratio = compressed.len() as f64 / original_length as f64 * 100.0;
        println!("Ratio:           {:.1}%", ratio);

        // Verify decompression
        let alice = ALICEText::default();
        match alice.decompress(&compressed) {
            Ok(_) => println!("Status:          Valid (decompression OK)"),
            Err(e) => println!("Status:          Invalid ({})", e),
        }
    }

    Ok(())
}

fn estimate_compression(input: &PathBuf, detailed: bool) -> Result<(), Box<dyn std::error::Error>> {
    let text = fs::read_to_string(input)?;
    let original_size = text.len();

    let estimator = EntropyEstimator::new();
    let estimate = estimator.estimate(&text);

    println!("Compression Estimate for: {}", input.display());
    println!("==========================");
    println!("Original Size:    {} bytes", original_size);
    println!("Estimated Size:   {} bytes", estimate.estimated_size);
    println!("Estimated Ratio:  {:.1}%", estimate.estimated_ratio * 100.0);
    println!("Space Savings:    {:.1}%", estimate.space_savings * 100.0);
    println!("Quality:          {}", estimate.quality());
    println!(
        "Compressible:     {}",
        if estimate.is_compressible() {
            "Yes"
        } else {
            "No"
        }
    );

    if detailed {
        println!();
        println!("Detailed Analysis:");
        println!(
            "  Shannon Entropy:   {:.2} bits/byte",
            estimate.shannon_entropy
        );
        println!(
            "  Pattern Coverage:  {:.1}%",
            estimate.pattern_coverage * 100.0
        );
        println!("  Repetition Score:  {:.2}", estimate.repetition_score);
        println!("  Unique Bytes:      {}", estimate.unique_bytes);
    }

    Ok(())
}

fn verify_file(input: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let compressed = fs::read(input)?;

    let alice = ALICEText::default();

    print!("Verifying {}... ", input.display());
    io::stdout().flush()?;

    match alice.decompress(&compressed) {
        Ok(text) => {
            println!("OK ({} bytes decompressed)", text.len());
        }
        Err(e) => {
            println!("FAILED");
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn compress_file_v3(
    input: &PathBuf,
    output: Option<PathBuf>,
    level: &str,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read input
    let text = fs::read_to_string(input)?;
    let original_size = text.len();

    // Parse compression level
    let compression_level = match level.to_lowercase().as_str() {
        "fast" => CompressionLevel::Fast,
        "balanced" => CompressionLevel::Balanced,
        "best" => CompressionLevel::Best,
        _ => {
            eprintln!("Unknown level: {}. Using balanced.", level);
            CompressionLevel::Balanced
        }
    };

    // Compress using v3 format
    let start = Instant::now();
    let compressed = compress_v3(&text, compression_level)?;
    let elapsed = start.elapsed();

    let compressed_size = compressed.len();

    // Write output
    let output_path = output.unwrap_or_else(|| {
        let mut p = input.clone();
        p.set_extension("atxt");
        p
    });

    fs::write(&output_path, &compressed)?;

    // Report
    let ratio = compressed_size as f64 / original_size as f64 * 100.0;
    let savings = 100.0 - ratio;

    if verbose {
        // Get metadata for detailed info
        let mut cursor = Cursor::new(&compressed);
        let metadata = FormatV3Metadata::read_from(&mut cursor)?;

        println!("ALICE-Text Compression (v3 - Columnar)");
        println!("======================================");
        println!("Input:      {}", input.display());
        println!("Output:     {}", output_path.display());
        println!("Level:      {:?}", compression_level);
        println!();
        println!("Original:   {} bytes", original_size);
        println!("Compressed: {} bytes", compressed_size);
        println!("Ratio:      {:.1}%", ratio);
        println!("Savings:    {:.1}%", savings);
        println!("Time:       {:.2}ms", elapsed.as_secs_f64() * 1000.0);
        println!();
        println!("Columns ({}):", metadata.columns.len());
        for col in &metadata.columns {
            if col.row_count > 0 {
                println!(
                    "  {:15} {:5} rows, {:6} bytes compressed",
                    col.col_type.name(),
                    col.row_count,
                    col.compressed_size
                );
            }
        }
    } else {
        println!(
            "{} -> {} ({:.1}% ratio, {:.1}% saved) [v3 queryable]",
            input.display(),
            output_path.display(),
            ratio,
            savings
        );
    }

    Ok(())
}

fn query_file(
    input: &PathBuf,
    show_columns: bool,
    show_stats: bool,
    select: Option<String>,
    filter: Option<String>,
    format: &str,
    limit: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check file format version (read header only)
    let data = fs::read(input)?;
    if data.len() < 10 {
        eprintln!("Error: File too small");
        return Ok(());
    }

    if &data[0..8] != b"ALICETXT" {
        eprintln!("Error: Invalid ALICE-Text file");
        return Ok(());
    }

    if data[8] < 3 {
        eprintln!("Error: Query requires v3 format. Use 'alice-text compress-v3' to create queryable files.");
        return Ok(());
    }

    // Use memory-mapped file for zero-copy access
    let engine = QueryEngine::open(input)?;

    // Show columns only
    if show_columns {
        println!("Available columns:");
        for col in engine.columns() {
            println!("  {}", col);
        }
        return Ok(());
    }

    // Show stats only
    if show_stats {
        let stats = engine.stats();
        println!("File Statistics");
        println!("===============");
        println!("Original size:     {} bytes", stats.original_size);
        println!("Compressed size:   {} bytes", stats.compressed_size);
        println!("Compression ratio: {:.1}%", stats.compression_ratio * 100.0);
        println!("Row count:         {}", stats.row_count);
        println!("Column count:      {}", stats.column_count);
        println!();
        println!("Columns:");
        for col in &stats.columns {
            println!(
                "  {:15} {:5} rows, {:6} bytes",
                col.name, col.row_count, col.compressed_size
            );
        }
        return Ok(());
    }

    // Parse select columns
    let select_cols: Vec<&str> = select
        .as_ref()
        .map(|s| s.split(',').map(|c| c.trim()).collect())
        .unwrap_or_else(|| vec!["log_levels", "ipv4", "timestamps"]);

    // Parse filter
    let (filter_col, filter_op, filter_val) = if let Some(ref f) = filter {
        parse_filter(f)?
    } else {
        (None, None, None)
    };

    // Execute query
    let result = if let (Some(col), Some(op), Some(val)) = (filter_col, filter_op, filter_val) {
        engine.query(&select_cols, col, op, val)?
    } else {
        engine.select_columns(&select_cols)?
    };

    // Apply limit
    let rows: Vec<_> = if let Some(n) = limit {
        result.rows.iter().take(n).collect()
    } else {
        result.rows.iter().collect()
    };

    // Output
    match format {
        "csv" => {
            println!("{}", result.columns.join(","));
            for row in rows {
                let values: Vec<&str> = result
                    .columns
                    .iter()
                    .map(|c| row.values.get(c).map(|s| s.as_str()).unwrap_or(""))
                    .collect();
                println!("{}", values.join(","));
            }
        }
        "json" => {
            println!("[");
            for (i, row) in rows.iter().enumerate() {
                let pairs: Vec<String> = result
                    .columns
                    .iter()
                    .filter_map(|c| row.values.get(c).map(|v| format!("\"{}\":\"{}\"", c, v)))
                    .collect();
                let comma = if i < rows.len() - 1 { "," } else { "" };
                println!("  {{{}}}{}", pairs.join(","), comma);
            }
            println!("]");
        }
        _ => {
            // Table format
            println!("{}", result.columns.join("\t"));
            println!("{}", "-".repeat(result.columns.len() * 20));
            for row in rows {
                let values: Vec<&str> = result
                    .columns
                    .iter()
                    .map(|c| row.values.get(c).map(|s| s.as_str()).unwrap_or(""))
                    .collect();
                println!("{}", values.join("\t"));
            }
            println!();
            println!("({} rows)", result.len());
        }
    }

    Ok(())
}

type FilterResult<'a> =
    Result<(Option<&'a str>, Option<Op>, Option<&'a str>), Box<dyn std::error::Error>>;

fn parse_filter(filter: &str) -> FilterResult<'_> {
    // Parse: column=value, column!=value, column>=value, column<=value, column>value, column<value, column~value
    // Order matters: check multi-char operators first
    if let Some((col, val)) = filter.split_once("!=") {
        Ok((Some(col.trim()), Some(Op::Ne), Some(val.trim())))
    } else if let Some((col, val)) = filter.split_once(">=") {
        Ok((Some(col.trim()), Some(Op::Ge), Some(val.trim())))
    } else if let Some((col, val)) = filter.split_once("<=") {
        Ok((Some(col.trim()), Some(Op::Le), Some(val.trim())))
    } else if let Some((col, val)) = filter.split_once(">") {
        Ok((Some(col.trim()), Some(Op::Gt), Some(val.trim())))
    } else if let Some((col, val)) = filter.split_once("<") {
        Ok((Some(col.trim()), Some(Op::Lt), Some(val.trim())))
    } else if let Some((col, val)) = filter.split_once("~") {
        Ok((Some(col.trim()), Some(Op::Contains), Some(val.trim())))
    } else if let Some((col, val)) = filter.split_once("=") {
        Ok((Some(col.trim()), Some(Op::Eq), Some(val.trim())))
    } else {
        Err(format!(
            "Invalid filter format: {}. Use column=value, column>=value, etc.",
            filter
        )
        .into())
    }
}
