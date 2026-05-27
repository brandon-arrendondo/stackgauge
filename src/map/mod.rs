pub mod arm_keil;
pub mod gnu_ld;
pub mod keil_lx51;

use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum MapFormat {
    GnuLd,
    ArmKeil,
    EspIdf,
    KeilC51,
}

impl std::fmt::Display for MapFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MapFormat::GnuLd => write!(f, "GNU ld"),
            MapFormat::ArmKeil => write!(f, "ARM/Keil MDK"),
            MapFormat::EspIdf => write!(f, "ESP-IDF (Xtensa/GNU ld)"),
            MapFormat::KeilC51 => write!(f, "Keil C51/LX51 (8051)"),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub address: u64,
    pub size: u64,
    pub section: String,
    pub object_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CallChainEntry {
    pub name: String,
    pub frame_size: u64,
}

#[derive(Debug)]
pub struct MaxStackInfo {
    pub bytes: u64,
    pub unknown_factors: Vec<String>,
    pub chain: Vec<CallChainEntry>,
}

impl MaxStackInfo {
    pub fn max_depth(&self) -> usize {
        self.chain.len()
    }
}

#[derive(Debug, Clone)]
pub struct CallNode {
    pub frame_size: u64,
    pub callees: Vec<String>,
}

#[derive(Debug)]
pub struct MapData {
    pub format: MapFormat,
    pub symbols: Vec<Symbol>,
    pub max_stack: Option<MaxStackInfo>,
}

pub fn detect_format(content: &str, hint: Option<&str>) -> MapFormat {
    if let Some(h) = hint {
        match h {
            "arm-keil" | "arm_keil" => return MapFormat::ArmKeil,
            "gnu-ld" | "gnu_ld" | "gnu" => return MapFormat::GnuLd,
            "esp-idf" | "esp_idf" | "esp32" | "xtensa" => return MapFormat::EspIdf,
            "keil-c51" | "keil_c51" | "c51" | "lx51" | "8051" => return MapFormat::KeilC51,
            _ => {}
        }
    }

    if content.contains("LX51 LINKER") || content.contains("OVERLAY MAP OF MODULE") {
        return MapFormat::KeilC51;
    }

    if content.contains("ARM Linker")
        || content.contains("armlink")
        || (content.contains(
            "==============================================================================",
        ) && content.contains("Image Symbol Table"))
    {
        return MapFormat::ArmKeil;
    }

    if content.contains("esp_idf") || content.contains("esp32") || content.contains("xtensa") {
        return MapFormat::EspIdf;
    }

    MapFormat::GnuLd
}

pub fn parse_map(path: &Path, hint: Option<&str>) -> Result<MapData> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read map file: {}", path.display()))?;

    let format = detect_format(&content, hint);

    match format {
        MapFormat::ArmKeil => arm_keil::parse(&content, format),
        MapFormat::GnuLd | MapFormat::EspIdf => gnu_ld::parse(&content, format),
        MapFormat::KeilC51 => keil_lx51::parse(&content, format),
    }
}
