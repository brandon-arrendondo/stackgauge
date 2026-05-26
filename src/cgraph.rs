use crate::map::{CallChainEntry, CallNode, MaxStackInfo};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Parse the content of a single GCC -fdump-ipa-cgraph dump file.
/// Returns a map of caller function name → list of callee function names.
pub fn parse_cgraph(content: &str) -> HashMap<String, Vec<String>> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut current: Option<String> = None;

    for line in content.lines() {
        if let Some(name) = parse_node_line(line) {
            graph.entry(name.clone()).or_default();
            current = Some(name);
            continue;
        }
        if let Some(callees) = parse_calls_line(line) {
            if let Some(ref caller) = current {
                let entry = graph.entry(caller.clone()).or_default();
                for callee in callees {
                    if !entry.contains(&callee) {
                        entry.push(callee);
                    }
                }
            }
        }
    }

    graph
}

/// Walk dirs for files whose name ends with `.cgraph` (e.g. `foo.c.001i.cgraph`).
pub fn collect_cgraph_files(dirs: &[&Path], exclude_dirs: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in dirs {
        for entry in WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_excluded_dir(e, exclude_dirs))
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file()
                && entry
                    .path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".cgraph"))
                    .unwrap_or(false)
            {
                files.push(entry.path().to_path_buf());
            }
        }
    }
    files
}

fn is_excluded_dir(entry: &walkdir::DirEntry, exclude_dirs: &[String]) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    let name = entry.file_name().to_str().unwrap_or("");
    exclude_dirs.iter().any(|ex| ex == name)
}

/// Load and merge call graphs from multiple dump files.
pub fn load_cgraph(files: &[PathBuf]) -> HashMap<String, Vec<String>> {
    let mut merged: HashMap<String, Vec<String>> = HashMap::new();
    for path in files {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                for (caller, callees) in parse_cgraph(&content) {
                    let entry = merged.entry(caller).or_default();
                    for callee in callees {
                        if !entry.contains(&callee) {
                            entry.push(callee);
                        }
                    }
                }
            }
            Err(e) => eprintln!("warning: failed to read {}: {}", path.display(), e),
        }
    }
    merged
}

/// Build a MaxStackInfo from a call graph and per-function frame sizes.
/// Finds the call chain with the highest total stack byte usage via DFS.
/// Frame sizes for functions absent from `su_frames` are treated as 0.
pub fn build_max_stack(
    cgraph: &HashMap<String, Vec<String>>,
    su_frames: &HashMap<String, u64>,
) -> Option<MaxStackInfo> {
    if cgraph.is_empty() {
        return None;
    }

    // Build CallNode graph: edges from cgraph, frame sizes from .su data
    let mut graph: HashMap<String, CallNode> = HashMap::new();
    for (caller, callees) in cgraph {
        graph
            .entry(caller.clone())
            .and_modify(|n| n.callees.clone_from(callees))
            .or_insert_with(|| CallNode {
                frame_size: su_frames.get(caller).copied().unwrap_or(0),
                callees: callees.clone(),
            });
        for callee in callees {
            graph.entry(callee.clone()).or_insert_with(|| CallNode {
                frame_size: su_frames.get(callee).copied().unwrap_or(0),
                callees: Vec::new(),
            });
        }
    }

    // Roots are nodes not called by anyone
    let all_callees: HashSet<&str> = graph
        .values()
        .flat_map(|n| n.callees.iter().map(|s| s.as_str()))
        .collect();
    let roots: Vec<String> = graph
        .keys()
        .filter(|k| !all_callees.contains(k.as_str()))
        .cloned()
        .collect();

    let mut best_chain: Vec<CallChainEntry> = Vec::new();
    let mut best_bytes: u64 = 0;

    for root in &roots {
        let chain = dfs_max_bytes(root, &graph, &mut HashSet::new());
        if chain.is_empty() {
            continue;
        }
        let chain_bytes: u64 = chain.iter().map(|e| e.frame_size).sum();
        if best_chain.is_empty() || chain_bytes > best_bytes {
            best_bytes = chain_bytes;
            best_chain = chain;
        }
    }

    if best_chain.is_empty() {
        return None;
    }

    Some(MaxStackInfo {
        bytes: best_bytes,
        unknown_factors: Vec::new(),
        chain: best_chain,
    })
}

/// DFS that maximises total frame bytes along the path.
fn dfs_max_bytes(
    name: &str,
    graph: &HashMap<String, CallNode>,
    visited: &mut HashSet<String>,
) -> Vec<CallChainEntry> {
    if !visited.insert(name.to_string()) {
        return Vec::new(); // cycle guard
    }

    let frame = graph.get(name).map(|n| n.frame_size).unwrap_or(0);

    let mut best_suffix: Vec<CallChainEntry> = Vec::new();
    let mut best_suffix_bytes: u64 = 0;

    if let Some(node) = graph.get(name) {
        for callee in &node.callees {
            let suffix = dfs_max_bytes(callee, graph, visited);
            let suffix_bytes: u64 = suffix.iter().map(|e| e.frame_size).sum();
            if suffix_bytes > best_suffix_bytes {
                best_suffix_bytes = suffix_bytes;
                best_suffix = suffix;
            }
        }
    }

    visited.remove(name);

    let mut chain = vec![CallChainEntry {
        name: name.to_string(),
        frame_size: frame,
    }];
    chain.extend(best_suffix);
    chain
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

/// Matches function node lines: `funcname/N (alias) ...`
///
/// GCC 9.x "Initial Symbol table" format puts nodes at column 0.
/// Older "Printing the call graph" format indents them. Both are accepted.
fn parse_node_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let slash = trimmed.find('/')?;
    let name = &trimmed[..slash];
    if !is_valid_ident(name) {
        return None;
    }
    let after_slash = &trimmed[slash + 1..];
    let digits_end = after_slash.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None; // nothing between / and next token
    }
    // Must be followed by a space then '(' for the alias
    let after_num = after_slash[digits_end..].trim_start();
    if after_num.starts_with('(') {
        Some(name.to_string())
    } else {
        None
    }
}

/// Matches `Calls: callee1/N callee2/M ...` (empty Calls: → empty vec)
fn parse_calls_line(line: &str) -> Option<Vec<String>> {
    let rest = line.trim().strip_prefix("Calls:")?;
    let rest = rest.trim();
    if rest.is_empty() {
        return Some(Vec::new());
    }
    let callees = rest
        .split_whitespace()
        .filter_map(|token| {
            let slash = token.rfind('/')?;
            let name = &token[..slash];
            if is_valid_ident(name) {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect();
    Some(callees)
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '$')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn su(entries: &[(&str, u64)]) -> HashMap<String, u64> {
        entries.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn test_parse_fixture_nodes() {
        let content = std::fs::read_to_string("tests/fixtures/sample.cgraph").unwrap();
        let graph = parse_cgraph(&content);
        for name in &["main", "process_data", "helper", "deep_leaf"] {
            assert!(graph.contains_key(*name), "missing node '{name}'");
        }
        assert_eq!(graph.len(), 4);
    }

    #[test]
    fn test_parse_fixture_edges() {
        let content = std::fs::read_to_string("tests/fixtures/sample.cgraph").unwrap();
        let graph = parse_cgraph(&content);
        assert_eq!(graph["main"], vec!["process_data", "helper"]);
        assert_eq!(graph["process_data"], vec!["deep_leaf"]);
        assert!(graph["helper"].is_empty());
        assert!(graph["deep_leaf"].is_empty());
    }

    #[test]
    fn test_comment_and_header_lines_ignored() {
        let content = std::fs::read_to_string("tests/fixtures/sample.cgraph").unwrap();
        let graph = parse_cgraph(&content);
        // ";; Function main" and "Printing the call graph." must not become nodes
        assert!(!graph.contains_key("Printing"));
        assert!(!graph.contains_key(";; Function main"));
    }

    #[test]
    fn test_build_max_stack_chain() {
        let content = std::fs::read_to_string("tests/fixtures/sample.cgraph").unwrap();
        let graph = parse_cgraph(&content);
        // main(32) → process_data(256) → deep_leaf(64) = 352
        // main(32) → helper(16)                         =  48
        let frames = su(&[("main", 32), ("process_data", 256), ("helper", 16), ("deep_leaf", 64)]);
        let ms = build_max_stack(&graph, &frames).unwrap();
        assert_eq!(ms.bytes, 352);
        assert_eq!(ms.chain.len(), 3);
        assert_eq!(ms.chain[0].name, "main");
        assert_eq!(ms.chain[1].name, "process_data");
        assert_eq!(ms.chain[2].name, "deep_leaf");
    }

    #[test]
    fn test_build_max_stack_bytes_matches_chain_sum() {
        let content = std::fs::read_to_string("tests/fixtures/sample.cgraph").unwrap();
        let graph = parse_cgraph(&content);
        let frames = su(&[("main", 32), ("process_data", 256), ("helper", 16), ("deep_leaf", 64)]);
        let ms = build_max_stack(&graph, &frames).unwrap();
        let total: u64 = ms.chain.iter().map(|e| e.frame_size).sum();
        assert_eq!(total, ms.bytes);
    }

    #[test]
    fn test_missing_su_frames_default_to_zero() {
        let content = std::fs::read_to_string("tests/fixtures/sample.cgraph").unwrap();
        let graph = parse_cgraph(&content);
        let ms = build_max_stack(&graph, &HashMap::new()).unwrap();
        assert_eq!(ms.bytes, 0);
        assert!(!ms.chain.is_empty());
    }

    #[test]
    fn test_empty_graph_returns_none() {
        assert!(build_max_stack(&HashMap::new(), &HashMap::new()).is_none());
    }

    #[test]
    fn test_called_by_lines_not_parsed_as_edges() {
        // "Called by:" lines must not add reverse edges
        let content = r#"
  foo/0 (foo) @src/foo.c:1:1
    Called by: bar/1
    Calls: baz/2

  baz/2 (baz) @src/foo.c:10:1
    Called by: foo/0
    Calls:
"#;
        let graph = parse_cgraph(content);
        // foo should only call baz, not bar
        let foo_calls = graph.get("foo").unwrap();
        assert_eq!(foo_calls.len(), 1);
        assert_eq!(foo_calls[0], "baz");
    }

    #[test]
    fn test_gcc9_initial_symbol_table_format() {
        // GCC 9.x emits node headers at column 0; Calls: is indented 2 spaces.
        let content = r#"
Initial Symbol table:

bar/1 (bar) @0x7f0000000000
  Type: function definition analyzed
  Called by:
  Calls: baz/2

baz/2 (baz) @0x7f0000000100
  Type: function definition analyzed
  Called by: bar/1
  Calls:
"#;
        let graph = parse_cgraph(content);
        assert!(graph.contains_key("bar"), "bar not parsed");
        assert!(graph.contains_key("baz"), "baz not parsed");
        assert_eq!(graph["bar"], vec!["baz"]);
        assert!(graph["baz"].is_empty());
    }

    #[test]
    fn test_merge_multiple_files() {
        // Two separate translation-unit dumps — load_cgraph must merge edges
        let file_a = r#"
  alpha/0 (alpha) @a.c:1:1
    Calls: beta/1
  beta/1 (beta) @a.c:10:1
    Calls:
"#;
        let file_b = r#"
  gamma/0 (gamma) @b.c:1:1
    Calls: alpha/0
"#;
        let mut a = parse_cgraph(file_a);
        for (k, v) in parse_cgraph(file_b) {
            let entry = a.entry(k).or_default();
            for callee in v {
                if !entry.contains(&callee) {
                    entry.push(callee);
                }
            }
        }
        assert_eq!(a["gamma"], vec!["alpha"]);
        assert_eq!(a["alpha"], vec!["beta"]);
        assert!(a["beta"].is_empty());
    }
}
