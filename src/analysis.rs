use crate::map::{MapData, MapFormat};
use crate::su::{FrameType, SuEntry};
use std::collections::HashMap;

#[derive(Debug)]
pub struct FunctionStackInfo {
    pub name: String,
    pub frame_size: u64,
    pub frame_type: FrameType,
    pub source_file: String,
    pub line: u32,
}

#[derive(Debug)]
pub struct AnalysisResult {
    pub format: MapFormat,
    pub map_file: String,
    pub su_file_count: usize,
    pub su_function_count: usize,

    /// Functions present in the final binary, with stack frame info
    pub functions: Vec<FunctionStackInfo>,

    /// Worst-case cumulative stack bytes (ARM/Keil call graph only)
    pub max_chain_bytes: Option<u64>,
    /// Deepest call chain (ARM/Keil call graph only)
    pub max_chain_depth: Option<usize>,
    /// Call chain entries for display
    pub max_chain: Vec<(String, u64)>,
    /// Whether the max stack includes unknown/dynamic factors
    pub max_chain_has_unknown: bool,
    pub max_chain_unknown_factors: Vec<String>,

    /// Threshold violations
    pub stack_threshold_exceeded: bool,
    pub depth_threshold_exceeded: bool,
}

pub struct AnalysisConfig {
    pub stack_threshold: Option<u64>,
    pub depth_threshold: Option<usize>,
}

pub fn run(
    map: &MapData,
    su_entries: &[SuEntry],
    su_file_count: usize,
    map_path: &str,
    cfg: &AnalysisConfig,
) -> AnalysisResult {
    // Build a lookup: function_name -> best SuEntry (largest frame wins for duplicates)
    let mut su_by_name: HashMap<String, &SuEntry> = HashMap::new();
    for entry in su_entries {
        su_by_name
            .entry(entry.function_name.clone())
            .and_modify(|existing| {
                if entry.frame_size > existing.frame_size {
                    *existing = entry;
                }
            })
            .or_insert(entry);
    }

    // For GNU ld / ESP-IDF: cross-reference map symbols with .su entries.
    // For ARM/Keil: use the symbol table from the map, augmented with .su data.
    let mut functions: Vec<FunctionStackInfo> = Vec::new();

    match map.format {
        MapFormat::ArmKeil => {
            // ARM/Keil map has a symbol table with sizes; augment with .su data
            for sym in &map.symbols {
                // Only care about code symbols (Thumb/ARM code)
                if let Some(su) = su_by_name.get(&sym.name) {
                    functions.push(FunctionStackInfo {
                        name: sym.name.clone(),
                        frame_size: su.frame_size,
                        frame_type: su.frame_type.clone(),
                        source_file: su.source_file.clone(),
                        line: su.line,
                    });
                }
                // If no .su info available, skip (ARM/Keil call graph covers the worst case)
            }

            // Also include any .su entries not in the symbol table
            for (name, su) in &su_by_name {
                if !map.symbols.iter().any(|s| &s.name == name) {
                    functions.push(FunctionStackInfo {
                        name: name.clone(),
                        frame_size: su.frame_size,
                        frame_type: su.frame_type.clone(),
                        source_file: su.source_file.clone(),
                        line: su.line,
                    });
                }
            }
        }

        MapFormat::KeilC51 => {
            // Overlay map gives us the complete function list with XDATA frame sizes.
            // No .su files needed — use map symbols directly.
            for sym in &map.symbols {
                // Only include functions that have a non-zero frame or appear in the call graph
                if sym.size > 0 || sym.name.contains('/') {
                    functions.push(FunctionStackInfo {
                        name: sym.name.clone(),
                        frame_size: sym.size,
                        frame_type: crate::su::FrameType::Static,
                        source_file: sym.object_file.clone().unwrap_or_default(),
                        line: 0,
                    });
                }
            }
        }

        MapFormat::GnuLd | MapFormat::EspIdf => {
            // Build set of symbol names from map for filtering
            let map_symbols: std::collections::HashSet<&str> =
                map.symbols.iter().map(|s| s.name.as_str()).collect();

            if map.symbols.is_empty() {
                // No map symbols parsed — include all .su entries
                for (name, su) in &su_by_name {
                    functions.push(FunctionStackInfo {
                        name: name.clone(),
                        frame_size: su.frame_size,
                        frame_type: su.frame_type.clone(),
                        source_file: su.source_file.clone(),
                        line: su.line,
                    });
                }
            } else {
                for (name, su) in &su_by_name {
                    if map_symbols.contains(name.as_str()) {
                        functions.push(FunctionStackInfo {
                            name: name.clone(),
                            frame_size: su.frame_size,
                            frame_type: su.frame_type.clone(),
                            source_file: su.source_file.clone(),
                            line: su.line,
                        });
                    }
                }
            }
        }
    }

    functions.sort_by(|a, b| b.frame_size.cmp(&a.frame_size));

    // Extract call graph analysis from ARM/Keil map
    let (max_chain_bytes, max_chain_depth, max_chain, has_unknown, unknown_factors) =
        if let Some(ms) = &map.max_stack {
            let chain: Vec<(String, u64)> = ms
                .chain
                .iter()
                .map(|e| (e.name.clone(), e.frame_size))
                .collect();
            let depth = ms.max_depth();
            (
                Some(ms.bytes),
                Some(depth),
                chain,
                !ms.unknown_factors.is_empty(),
                ms.unknown_factors.clone(),
            )
        } else {
            (None, None, Vec::new(), false, Vec::new())
        };

    // Threshold checks
    let stack_threshold_exceeded = cfg
        .stack_threshold
        .map(|t| {
            // Use call graph max if available, otherwise max single frame
            let worst = max_chain_bytes
                .or_else(|| functions.first().map(|f| f.frame_size))
                .unwrap_or(0);
            worst > t
        })
        .unwrap_or(false);

    let depth_threshold_exceeded = cfg
        .depth_threshold
        .map(|t| max_chain_depth.map(|d| d > t).unwrap_or(false))
        .unwrap_or(false);

    AnalysisResult {
        format: map.format.clone(),
        map_file: map_path.to_string(),
        su_file_count,
        su_function_count: su_entries.len(),
        functions,
        max_chain_bytes,
        max_chain_depth,
        max_chain,
        max_chain_has_unknown: has_unknown,
        max_chain_unknown_factors: unknown_factors,
        stack_threshold_exceeded,
        depth_threshold_exceeded,
    }
}
