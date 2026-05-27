use super::{CallChainEntry, CallNode, MapData, MapFormat, MaxStackInfo, Symbol};
use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

static MAX_STACK_RE: OnceLock<Regex> = OnceLock::new();
static CHAIN_ENTRY_RE: OnceLock<Regex> = OnceLock::new();
static SYMBOL_RE: OnceLock<Regex> = OnceLock::new();

fn max_stack_re() -> &'static Regex {
    MAX_STACK_RE.get_or_init(|| Regex::new(r"Maximum Stack Usage\s*=\s*(\d+)\s+bytes(.*)").unwrap())
}

fn chain_entry_re() -> &'static Regex {
    CHAIN_ENTRY_RE.get_or_init(|| {
        Regex::new(r"^( +)([a-zA-Z_][a-zA-Z0-9_:.$@<>~]*)\s+\[(\d+)\](.*)$").unwrap()
    })
}

fn symbol_re() -> &'static Regex {
    SYMBOL_RE.get_or_init(|| {
        // "    funcName        0x08001234   Thumb Code     48  file.o(section)"
        Regex::new(
            r"^\s+([a-zA-Z_][a-zA-Z0-9_:.$@<>~]*)\s+(0x[0-9a-fA-F]+)\s+\S+\s+\S+\s+(\d+)\s+(.+)$",
        )
        .unwrap()
    })
}

#[derive(PartialEq)]
enum Section {
    Other,
    SymbolTable,
    CallGraph,
    CallChain,
}

pub fn parse(content: &str, format: MapFormat) -> Result<MapData> {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut max_stack: Option<MaxStackInfo> = None;
    let mut call_graph: HashMap<String, CallNode> = HashMap::new();

    let mut section = Section::Other;
    let mut chain_lines: Vec<(usize, String, u64)> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "Image Symbol Table" || trimmed.contains("Global Symbols") {
            section = Section::SymbolTable;
            continue;
        }
        if trimmed == "Call Graph" {
            section = Section::CallGraph;
            continue;
        }
        if trimmed.starts_with("=====") {
            if section == Section::CallChain {
                finalise_chain(&chain_lines, &mut call_graph);
                chain_lines.clear();
            }
            section = Section::Other;
            continue;
        }

        match section {
            Section::SymbolTable => {
                if let Some(caps) = symbol_re().captures(trimmed) {
                    let name = caps[1].to_string();
                    let addr = u64::from_str_radix(&caps[2][2..], 16).unwrap_or(0);
                    let size: u64 = caps[3].parse().unwrap_or(0);
                    let obj = caps[4].to_string();
                    let (obj_file, sec) = if let Some(pos) = obj.find('(') {
                        let o = obj[..pos].to_string();
                        let s = obj[pos + 1..].trim_end_matches(')').to_string();
                        (Some(o), s)
                    } else {
                        (Some(obj), String::new())
                    };
                    symbols.push(Symbol {
                        name,
                        address: addr,
                        size,
                        section: sec,
                        object_file: obj_file,
                    });
                }
            }

            Section::CallGraph | Section::CallChain => {
                if let Some(caps) = max_stack_re().captures(trimmed) {
                    let bytes: u64 = caps[1].parse().unwrap_or(0);
                    let rest = caps[2].trim();
                    let unknown_factors = parse_unknown_factors(rest);
                    max_stack = Some(MaxStackInfo {
                        bytes,
                        unknown_factors,
                        chain: Vec::new(),
                    });
                    continue;
                }

                if trimmed == "Call chain for Maximum Stack Usage:" {
                    if section == Section::CallChain {
                        finalise_chain(&chain_lines, &mut call_graph);
                        chain_lines.clear();
                    }
                    section = Section::CallChain;
                    continue;
                }

                if section == Section::CallChain {
                    if let Some(caps) = chain_entry_re().captures(line) {
                        let indent = caps[1].len();
                        let name = caps[2].to_string();
                        let frame: u64 = caps[3].parse().unwrap_or(0);
                        chain_lines.push((indent, name, frame));
                    } else if trimmed.is_empty() && !chain_lines.is_empty() {
                        finalise_chain(&chain_lines, &mut call_graph);
                        chain_lines.clear();
                    }
                }
            }

            Section::Other => {}
        }
    }

    if !chain_lines.is_empty() {
        finalise_chain(&chain_lines, &mut call_graph);
    }

    if let Some(ref mut ms) = max_stack {
        let chain = build_max_chain(&call_graph);
        if !chain.is_empty() {
            ms.chain = chain;
        }
    }

    symbols.sort_by(|a, b| a.address.cmp(&b.address));

    Ok(MapData {
        format,
        symbols,
        max_stack,
    })
}

fn parse_unknown_factors(rest: &str) -> Vec<String> {
    if let Some(inner) = rest
        .trim_start_matches('+')
        .trim()
        .strip_prefix("Unknown(")
        .and_then(|s| s.strip_suffix(')'))
    {
        inner
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        Vec::new()
    }
}

fn finalise_chain(chain: &[(usize, String, u64)], graph: &mut HashMap<String, CallNode>) {
    for i in 0..chain.len() {
        let (indent, name, frame) = &chain[i];
        let node = graph.entry(name.clone()).or_insert_with(|| CallNode {
            frame_size: *frame,
            callees: Vec::new(),
        });
        node.frame_size = *frame;

        if let Some((_, callee_name, _)) = chain[i + 1..]
            .iter()
            .find(|(next_indent, _, _)| *next_indent > *indent)
        {
            if !node.callees.contains(callee_name) {
                node.callees.push(callee_name.clone());
            }
        }
    }
}

fn build_max_chain(graph: &HashMap<String, CallNode>) -> Vec<CallChainEntry> {
    if graph.is_empty() {
        return Vec::new();
    }

    let all_callees: std::collections::HashSet<&str> = graph
        .values()
        .flat_map(|n| n.callees.iter().map(|s| s.as_str()))
        .collect();

    let roots: Vec<&str> = graph
        .keys()
        .filter(|k| !all_callees.contains(k.as_str()))
        .map(|s| s.as_str())
        .collect();

    let mut best: Vec<CallChainEntry> = Vec::new();

    for root in roots {
        let chain = dfs_max_depth(root, graph, &mut std::collections::HashSet::new());
        if chain.len() > best.len() {
            best = chain;
        }
    }

    best
}

fn dfs_max_depth(
    name: &str,
    graph: &HashMap<String, CallNode>,
    visited: &mut std::collections::HashSet<String>,
) -> Vec<CallChainEntry> {
    if !visited.insert(name.to_string()) {
        return Vec::new();
    }

    let frame = graph.get(name).map(|n| n.frame_size).unwrap_or(0);

    let mut best_suffix: Vec<CallChainEntry> = Vec::new();

    if let Some(node) = graph.get(name) {
        for callee in &node.callees {
            let suffix = dfs_max_depth(callee, graph, visited);
            if suffix.len() > best_suffix.len() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_parsing() {
        let content = std::fs::read_to_string("tests/fixtures/arm_keil.map").unwrap();
        let result = parse(&content, MapFormat::ArmKeil).unwrap();
        let ms = result.max_stack.expect("should have max_stack");
        assert_eq!(ms.bytes, 304);
        assert_eq!(ms.chain.len(), 4, "expected 4-deep call chain");
        assert_eq!(ms.chain[0].name, "main");
        assert_eq!(ms.chain[3].name, "deep_leaf");
    }
}
