use anyhow::{Context, Result};
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq)]
pub enum FrameType {
    Static,
    Dynamic,
    DynamicBounded,
    Unknown(String),
}

impl std::fmt::Display for FrameType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameType::Static => write!(f, "static"),
            FrameType::Dynamic => write!(f, "dynamic"),
            FrameType::DynamicBounded => write!(f, "dynamic,bounded"),
            FrameType::Unknown(s) => write!(f, "{}", s),
        }
    }
}

impl FrameType {
    pub fn is_bounded(&self) -> bool {
        matches!(self, FrameType::Static | FrameType::DynamicBounded)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SuEntry {
    pub source_file: String,
    pub line: u32,
    pub col: u32,
    pub function_name: String,
    pub frame_size: u64,
    pub frame_type: FrameType,
}

pub fn parse_su_file(path: &Path) -> Result<Vec<SuEntry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read .su file: {}", path.display()))?;

    let mut entries = Vec::new();
    for (lineno, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_su_line(line) {
            Some(entry) => entries.push(entry),
            None => {
                eprintln!(
                    "warning: {}:{}: unparseable .su line: {}",
                    path.display(),
                    lineno + 1,
                    line
                );
            }
        }
    }
    Ok(entries)
}

fn parse_su_line(line: &str) -> Option<SuEntry> {
    // Format: path/to/file.c:line:col:function_name\tframe_size\ttype
    let (location, rest) = line.split_once('\t')?;
    let (frame_str, type_str) = rest.split_once('\t')?;

    let frame_size: u64 = frame_str.trim().parse().ok()?;

    let frame_type = match type_str.trim() {
        "static" => FrameType::Static,
        "dynamic" => FrameType::Dynamic,
        "dynamic,bounded" => FrameType::DynamicBounded,
        other => FrameType::Unknown(other.to_string()),
    };

    // Split location into file:line:col:func — last colon-segment is the function name
    // The file path itself may contain colons on Windows but we split from right.
    let parts: Vec<&str> = location.splitn(4, ':').collect();
    if parts.len() < 4 {
        // Try 3-part fallback: some versions omit the column
        if parts.len() == 3 {
            return Some(SuEntry {
                source_file: parts[0].to_string(),
                line: parts[1].parse().ok()?,
                col: 0,
                function_name: parts[2].to_string(),
                frame_size,
                frame_type,
            });
        }
        return None;
    }

    Some(SuEntry {
        source_file: parts[0].to_string(),
        line: parts[1].parse().ok()?,
        col: parts[2].parse().ok()?,
        function_name: parts[3].to_string(),
        frame_size,
        frame_type,
    })
}

pub fn collect_su_files(dirs: &[&Path]) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for dir in dirs {
        for entry in WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file()
                && entry.path().extension().and_then(|e| e.to_str()) == Some("su")
            {
                files.push(entry.path().to_path_buf());
            }
        }
    }
    files
}

pub fn load_su_entries(su_files: &[std::path::PathBuf]) -> Vec<SuEntry> {
    let mut entries = Vec::new();
    for path in su_files {
        match parse_su_file(path) {
            Ok(mut parsed) => entries.append(&mut parsed),
            Err(e) => eprintln!("warning: {:#}", e),
        }
    }
    entries
}
