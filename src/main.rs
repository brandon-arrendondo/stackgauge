mod analysis;
mod cgraph;
mod config;
mod map;
mod su;

use analysis::{AnalysisConfig, AnalysisResult};
use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use colored::Colorize;
use config::Config;
use std::path::PathBuf;
use std::process;

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

/// Static stack depth analysis for embedded system map files.
///
/// Parses GNU ld, ESP-IDF (Xtensa), and ARM/Keil linker map files together
/// with GCC .su (stack usage) files to report per-function frame sizes and,
/// where call-graph data is available, worst-case cumulative stack depth.
///
/// Exit codes:
///   0  all metrics within threshold
///   1  threshold exceeded (pre-commit fails here)
///   2  fatal error (parse failure, missing file, etc.)
#[derive(Parser, Debug)]
#[command(
    version,
    args_override_self = true,
    verbatim_doc_comment
)]
struct Args {
    /// Map file to analyze
    map_file: PathBuf,

    /// Directory to search recursively for .su files (repeatable)
    #[arg(short = 's', long = "su-dir", value_name = "DIR")]
    su_dirs: Vec<PathBuf>,

    /// Explicit .su file to include (repeatable)
    #[arg(long = "su-file", value_name = "FILE")]
    su_files: Vec<PathBuf>,

    /// Directory to search recursively for GCC IPA cgraph dumps (repeatable)
    #[arg(long = "cgraph-dir", value_name = "DIR")]
    cgraph_dirs: Vec<PathBuf>,

    /// Explicit GCC IPA cgraph dump file to include (repeatable)
    #[arg(long = "cgraph-file", value_name = "FILE")]
    cgraph_files: Vec<PathBuf>,

    /// Fail if worst-case stack exceeds this many bytes
    #[arg(short = 't', long, value_name = "BYTES")]
    stack_threshold: Option<u64>,

    /// Fail if call depth exceeds this count (ARM/Keil call graph only)
    #[arg(long, value_name = "N")]
    depth_threshold: Option<usize>,

    /// Output format
    #[arg(short, long, default_value = "text")]
    format: OutputFormat,

    /// Skip directories with this name during .su / .cgraph search (repeatable)
    #[arg(long = "exclude-dir", value_name = "NAME")]
    exclude_dirs: Vec<String>,

    /// Number of functions to show (default 10); ignored when -v is set
    #[arg(long, value_name = "N")]
    top_n: Option<usize>,

    /// Show all functions, not just the top N worst
    #[arg(short, long)]
    verbose: bool,

    /// Config file [default: stackgauge.toml in current directory]
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Force map file format instead of auto-detecting
    #[arg(long, value_name = "TOOLCHAIN")]
    toolchain: Option<String>,
}

fn load_config(path: Option<&PathBuf>) -> Config {
    let candidate = path
        .cloned()
        .unwrap_or_else(|| PathBuf::from("stackgauge.toml"));

    match std::fs::read_to_string(&candidate) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!(
                "warning: failed to parse {}: {:#}",
                candidate.display(),
                e
            );
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

fn run(args: Args) -> Result<bool> {
    let cfg = load_config(args.config.as_ref());

    // Merge thresholds: CLI overrides config file
    let stack_threshold = args.stack_threshold.or(cfg.stack_threshold);
    let depth_threshold = args.depth_threshold.or(cfg.depth_threshold);
    let top_n = args.top_n.or(cfg.top_n).unwrap_or(10);

    // Build exclusion list: CLI + config + built-in CMake probe dirs
    let mut exclude_dirs: Vec<String> = args.exclude_dirs.clone();
    for d in &cfg.exclude_dirs {
        if !exclude_dirs.contains(d) {
            exclude_dirs.push(d.clone());
        }
    }
    for default in &["CompilerIdC", "CompilerIdCXX", "CompilerIdASM"] {
        let s = default.to_string();
        if !exclude_dirs.contains(&s) {
            exclude_dirs.push(s);
        }
    }

    // Resolve .su search directories from CLI + config
    let mut su_dirs: Vec<PathBuf> = args.su_dirs.clone();
    for d in &cfg.su_dirs {
        su_dirs.push(PathBuf::from(d));
    }

    // Collect .su files
    let dir_refs: Vec<&std::path::Path> = su_dirs.iter().map(|p| p.as_path()).collect();
    let mut su_file_paths: Vec<PathBuf> = su::collect_su_files(&dir_refs, &exclude_dirs);
    su_file_paths.extend(args.su_files.iter().cloned());
    let su_file_count = su_file_paths.len();
    let su_entries = su::load_su_entries(&su_file_paths);

    // Parse map file
    let toolchain_hint = args.toolchain.as_deref().or(cfg.toolchain.as_deref());

    let mut map_data = map::parse_map(&args.map_file, toolchain_hint)
        .with_context(|| format!("parsing {}", args.map_file.display()))?;

    // For GNU ld / ESP-IDF: load GCC IPA call graph dumps to enable depth analysis
    if matches!(map_data.format, map::MapFormat::GnuLd | map::MapFormat::EspIdf) {
        let cgraph_dir_refs: Vec<&std::path::Path> =
            args.cgraph_dirs.iter().map(|p| p.as_path()).collect();
        let mut cgraph_paths = cgraph::collect_cgraph_files(&cgraph_dir_refs, &exclude_dirs);
        cgraph_paths.extend(args.cgraph_files.iter().cloned());

        if !cgraph_paths.is_empty() {
            let graph = cgraph::load_cgraph(&cgraph_paths);
            if !graph.is_empty() {
                let su_frames: std::collections::HashMap<String, u64> = su_entries
                    .iter()
                    .map(|e| (e.function_name.clone(), e.frame_size))
                    .collect();
                if let Some(ms) = cgraph::build_max_stack(&graph, &su_frames) {
                    map_data.max_stack = Some(ms);
                }
            }
        }
    }

    // Run analysis
    let result = analysis::run(
        &map_data,
        &su_entries,
        su_file_count,
        &args.map_file.display().to_string(),
        &AnalysisConfig {
            stack_threshold,
            depth_threshold,
        },
    );

    match args.format {
        OutputFormat::Text => print_text(&result, args.verbose, top_n, stack_threshold, depth_threshold),
        OutputFormat::Json => print_json(&result)?,
    }

    let threshold_exceeded = result.stack_threshold_exceeded || result.depth_threshold_exceeded;
    Ok(threshold_exceeded)
}

fn main() {
    let args = Args::parse();
    match run(args) {
        Ok(exceeded) => {
            if exceeded {
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("{}: {:#}", "error".red().bold(), e);
            process::exit(2);
        }
    }
}

// ── Text output ───────────────────────────────────────────────────────────────

fn print_text(
    r: &AnalysisResult,
    verbose: bool,
    top_n: usize,
    stack_threshold: Option<u64>,
    depth_threshold: Option<usize>,
) {
    println!("stackgauge analysis");
    println!("{}", "=".repeat(60));
    println!("  Map file  : {}", r.map_file);
    println!("  Format    : {}", r.format);
    println!(
        "  SU files  : {} files, {} functions",
        r.su_file_count, r.su_function_count
    );

    if let Some(t) = stack_threshold {
        println!("  Stack thr : {} bytes", t);
    }
    if let Some(t) = depth_threshold {
        println!("  Depth thr : {} levels", t);
    }

    println!();

    // Per-function stack frame table
    if r.functions.is_empty() {
        println!("{}", "No function stack data found.".yellow());
        println!("  Ensure .su files are provided (--su-dir or --su-file).");
    } else {
        let dynamic_count = r
            .functions
            .iter()
            .filter(|f| !f.frame_type.is_bounded())
            .count();

        println!("Per-function stack frames");
        println!("{}", "-".repeat(60));

        let display_limit = if verbose { r.functions.len() } else { top_n };
        let col_w = 40usize;

        for func in r.functions.iter().take(display_limit) {
            let bounded = func.frame_type.is_bounded();
            let flag = if bounded {
                " ".normal()
            } else {
                "!".red().bold()
            };
            let size_str = format!("{:>6} bytes", func.frame_size);
            let size_colored = if !bounded {
                size_str.red().bold()
            } else if let Some(t) = stack_threshold {
                if func.frame_size > t {
                    size_str.red().bold()
                } else {
                    size_str.normal()
                }
            } else {
                size_str.normal()
            };

            println!(
                "  {} {:<col_w$} {} ({} : {})",
                flag,
                func.name,
                size_colored,
                func.frame_type,
                format!("{}:{}", func.source_file, func.line),
            );
        }

        if !verbose && r.functions.len() > display_limit {
            println!(
                "  ... {} more (use -v to show all)",
                r.functions.len() - display_limit
            );
        }

        if dynamic_count > 0 {
            println!();
            println!(
                "  {} {} function(s) with dynamic/unbounded stack frames",
                "!".red().bold(),
                dynamic_count
            );
        }
    }

    // Call graph analysis
    if let Some(chain_bytes) = r.max_chain_bytes {
        println!();
        let cg_label = match r.format {
            map::MapFormat::ArmKeil => "Call graph analysis (ARM/Keil MDK)",
            map::MapFormat::KeilC51 => "Call graph analysis (Keil C51/LX51 XDATA overlay)",
            _ => "Call graph analysis (GCC IPA cgraph)",
        };
        println!("{cg_label}");
        println!("{}", "-".repeat(60));

        let unknown_suffix = if r.max_chain_has_unknown {
            format!(
                " + Unknown({})",
                r.max_chain_unknown_factors.join(", ")
            )
        } else {
            String::new()
        };

        let bytes_label = format!(
            "  Maximum stack usage: {} bytes{}",
            chain_bytes, unknown_suffix
        );
        if let Some(t) = stack_threshold {
            if chain_bytes > t {
                println!("{}", bytes_label.red().bold());
            } else {
                println!("{}", bytes_label);
            }
        } else {
            println!("{}", bytes_label);
        }

        if let Some(depth) = r.max_chain_depth {
            let depth_label = format!("  Call depth          : {} levels", depth);
            if let Some(t) = depth_threshold {
                if depth > t {
                    println!("{}", depth_label.red().bold());
                } else {
                    println!("{}", depth_label);
                }
            } else {
                println!("{}", depth_label);
            }
        }

        if !r.max_chain.is_empty() {
            println!();
            println!("  Deepest call chain:");
            for (i, (name, frame)) in r.max_chain.iter().enumerate() {
                let indent = "  ".repeat(i + 1);
                let arrow = if i + 1 < r.max_chain.len() { " →" } else { "" };
                println!("    {}{} [{} bytes]{}", indent, name, frame, arrow);
            }
        }
    }

    // Threshold results
    println!();
    println!("{}", "-".repeat(60));

    let any_exceeded = r.stack_threshold_exceeded || r.depth_threshold_exceeded;

    if r.stack_threshold_exceeded {
        let worst = r
            .max_chain_bytes
            .or_else(|| r.functions.first().map(|f| f.frame_size))
            .unwrap_or(0);
        println!(
            "  {} Stack threshold exceeded: {} bytes > {} bytes",
            "FAIL".red().bold(),
            worst,
            stack_threshold.unwrap_or(0)
        );
    }

    if r.depth_threshold_exceeded {
        println!(
            "  {} Depth threshold exceeded: {} levels > {} levels",
            "FAIL".red().bold(),
            r.max_chain_depth.unwrap_or(0),
            depth_threshold.unwrap_or(0)
        );
    }

    if !any_exceeded {
        if stack_threshold.is_some() || depth_threshold.is_some() {
            println!("  {} All metrics within threshold", "PASS".green().bold());
        } else {
            println!("  {} No thresholds configured", "INFO".cyan());
        }
    }
}

// ── JSON output ───────────────────────────────────────────────────────────────

fn print_json(r: &AnalysisResult) -> Result<()> {
    use serde_json::{json, Value};

    let functions: Vec<Value> = r
        .functions
        .iter()
        .map(|f| {
            json!({
                "name": f.name,
                "frame_bytes": f.frame_size,
                "frame_type": f.frame_type.to_string(),
                "source": f.source_file,
                "line": f.line,
            })
        })
        .collect();

    let call_chain: Vec<Value> = r
        .max_chain
        .iter()
        .enumerate()
        .map(|(i, (name, frame))| json!({ "depth": i, "name": name, "frame_bytes": frame }))
        .collect();

    let out = json!({
        "map_file": r.map_file,
        "format": r.format.to_string(),
        "su_files": r.su_file_count,
        "su_functions": r.su_function_count,
        "functions": functions,
        "call_graph": {
            "available": r.max_chain_bytes.is_some(),
            "max_stack_bytes": r.max_chain_bytes,
            "max_depth": r.max_chain_depth,
            "has_unknown": r.max_chain_has_unknown,
            "unknown_factors": r.max_chain_unknown_factors,
            "chain": call_chain,
        },
        "threshold_exceeded": r.stack_threshold_exceeded || r.depth_threshold_exceeded,
        "stack_threshold_exceeded": r.stack_threshold_exceeded,
        "depth_threshold_exceeded": r.depth_threshold_exceeded,
    });

    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
