// Output map construction for firmion.
//
// MapDb collects the semantic payload for all map output formats. All data
// derives from the LocationDb and IRDb after layout_phase completes.
//

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::Diags;
use ir::IRKind;
use ir::ParameterValue;
use irdb::IRDb;
use irdb::locationdb::LocationDb;
use mapdb::{ConstEntry, LabelEntry, MapDb, SectionEntry};

#[allow(unused_imports)]
use tracing::{debug, error, info, trace, warn};

// -- Private formatting helpers -----------------------------------------------

/// Returns the minimum name-column width: at least 16, at least as wide as
/// the longest name in `names`.
fn name_col_width<'a>(names: impl Iterator<Item = &'a str>) -> usize {
    names.map(str::len).max().unwrap_or(0).max(16)
}

/// Renders a `ParameterValue` as a human-readable string.
///   U64      → 0x0000000000001000
///   I64      → -42
///   Integer  → 42
///   String   → "hello"
pub fn fmt_const_value(pv: &ParameterValue) -> String {
    match pv {
        ParameterValue::U64(v) => format!("0x{v:016x}"),
        ParameterValue::I64(v) => format!("{v}"),
        ParameterValue::Integer(v) => format!("{v}"),
        ParameterValue::QuotedString(s) => format!("\"{s}\""),
        ParameterValue::Identifier(s) | ParameterValue::DeferredRef(s) => s.clone(),
        ParameterValue::Extension => "(extension)".to_string(),
        ParameterValue::Unknown => "(unknown)".to_string(),
    }
}

// -- CSV formatter --------------------------------------------------

/// Renders `map` as a CSV map.
///
/// Format overview:
/// ```text
/// Output File, output.bin
/// Base Address, 0x0000000000001000
/// Total Size (hex), 0x0000000000000050
/// Total Size (decimal), 80
///
/// Constants
/// Name,            Value,
/// BASE,            0x0000000000001000,
///
/// Sections
/// Name,            Address,             Offset,              File Offset,         Size (bytes),
/// foo,             0x0000000000001000,  0x0000000000000000,  0x0000000000000000,  50,
///
/// Labels
/// Name,            Address,             Offset,              File Offset,
/// lab1,            0x0000000000001004,  0x0000000000000004,  0x0000000000000004,
/// ```
pub fn format_csv(map: &MapDb) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    // -- Header ----------------------------------------------------------------
    writeln!(out, "Output File, {}", map.output_file).unwrap();
    writeln!(out, "Base Address, 0x{:016x}", map.base_addr).unwrap();
    writeln!(out, "Total Size (hex), 0x{:016x}", map.total_size).unwrap();
    writeln!(out, "Total Size (decimal), {}", map.total_size).unwrap();

    // -- Constants -------------------------------------------------------------
    writeln!(out).unwrap();
    writeln!(out, "Constants").unwrap();
    if map.consts.is_empty() {
        writeln!(out, "  (none)").unwrap();
    } else {
        let name_w = name_col_width(map.consts.iter().map(|c| c.name.as_str()));
        writeln!(out, "{:<name_w$},  {:<20},  Used,", "Name,", "Value,").unwrap();
        for c in &map.consts {
            writeln!(
                out,
                "{:<name_w$},  {:<20},  {},",
                c.name,
                fmt_const_value(&c.value),
                if c.used { "yes" } else { "no" }
            )
            .unwrap();
        }
    }

    // -- Sections --------------------------------------------------------------
    writeln!(out).unwrap();
    writeln!(out, "Sections").unwrap();
    if map.sections.is_empty() {
        writeln!(out, "  (none)").unwrap();
    } else {
        let name_w = name_col_width(map.sections.iter().map(|s| s.name.as_str()));
        writeln!(
            out,
            "{:<name_w$},  {:<18},  {:<18},  {:<18},  Size (bytes),",
            "Name", "Address", "Offset", "File Offset"
        )
        .unwrap();
        for s in &map.sections {
            writeln!(
                out,
                "{:<name_w$},  0x{:016x},  0x{:016x},  0x{:016x},  {},",
                s.name, s.abs_start, s.off, s.file_offset, s.size
            )
            .unwrap();
        }
    }

    // -- Labels ----------------------------------------------------------------
    writeln!(out).unwrap();
    writeln!(out, "Labels").unwrap();
    if map.labels.is_empty() {
        writeln!(out, "  (none)").unwrap();
    } else {
        let name_w = name_col_width(map.labels.iter().map(|l| l.name.as_str()));
        writeln!(
            out,
            "{:<name_w$},  {:<18},  {:<18},  File Offset,",
            "Name,", "Address,", "Offset,"
        )
        .unwrap();
        for l in &map.labels {
            writeln!(
                out,
                "{:<name_w$},  0x{:016x},  0x{:016x},  0x{:016x},",
                l.name, l.abs_addr, l.off, l.file_offset
            )
            .unwrap();
        }
    }

    out
}

/// Produces a C99 preprocessor compatible `.h` header format output
pub fn format_c99(map: &MapDb) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    let stem = std::path::Path::new(&map.output_file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("OUTPUT")
        .to_uppercase()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "_");

    writeln!(out, "// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!").unwrap();
    writeln!(out, "// Automatically generated file! Do not edit!").unwrap();
    writeln!(out, "// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!").unwrap();
    writeln!(out, "#ifndef {}_MAP_H", stem).unwrap();
    writeln!(out, "#define {}_MAP_H\n", stem).unwrap();

    writeln!(
        out,
        "#define {}_MAP_BASE_ADDR 0x{:016x}ULL",
        stem, map.base_addr
    )
    .unwrap();
    writeln!(out, "#define {}_MAP_TOTAL_SIZE {}ULL", stem, map.total_size).unwrap();

    if !map.sections.is_empty() {
        writeln!(out, "\n// Sections").unwrap();
        for sec in &map.sections {
            let sec_name = sec.name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
            writeln!(
                out,
                "#define {}_MAP_{}_ADDR 0x{:016x}ULL",
                stem, sec_name, sec.abs_start
            )
            .unwrap();
            writeln!(
                out,
                "#define {}_MAP_{}_OFFSET 0x{:016x}ULL",
                stem, sec_name, sec.off
            )
            .unwrap();
            writeln!(
                out,
                "#define {}_MAP_{}_FILE_OFFSET 0x{:016x}ULL",
                stem, sec_name, sec.file_offset
            )
            .unwrap();
            writeln!(
                out,
                "#define {}_MAP_{}_SIZE {}ULL",
                stem, sec_name, sec.size
            )
            .unwrap();
            writeln!(out).unwrap();
        }
    }

    if !map.labels.is_empty() {
        writeln!(out, "// Labels").unwrap();
        for lab in &map.labels {
            let lab_name = lab.name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
            writeln!(
                out,
                "#define {}_MAP_{}_ADDR 0x{:016x}ULL",
                stem, lab_name, lab.abs_addr
            )
            .unwrap();
            writeln!(
                out,
                "#define {}_MAP_{}_OFFSET 0x{:016x}ULL",
                stem, lab_name, lab.off
            )
            .unwrap();
            writeln!(
                out,
                "#define {}_MAP_{}_FILE_OFFSET 0x{:016x}ULL",
                stem, lab_name, lab.file_offset
            )
            .unwrap();
            writeln!(out).unwrap();
        }
    }

    writeln!(out, "\n#endif").unwrap();
    out
}

// -- JSON formatter ------------------------------------------------------------

/// Renders `map` as a pretty-printed JSON string.
///
/// Addresses and offsets are hex strings (`"0x..."`) for readability.
/// Sizes and the total are plain JSON numbers.
/// Const values use the same string representation as `format_csv`.
///
/// ```json
/// {
///   "output_file": "output.bin",
///   "base_addr": "0x0000000000001000",
///   "total_size": 80,
///   "constants": [
///     { "name": "BASE", "value": "0x0000000000001000" }
///   ],
///   "sections": [
///     { "name": "text", "address": "0x0000000000001000",
///       "offset": "0x0000000000000000",
///       "file_offset": "0x0000000000000000", "size": 50 }
///   ],
///   "labels": [
///     { "name": "start", "address": "0x0000000000001000",
///       "offset": "0x0000000000000000",
///       "file_offset": "0x0000000000000000" }
///   ]
/// }
/// ```
pub fn format_json(map: &MapDb) -> String {
    use serde_json::{Value, json};

    let constants: Vec<Value> = map
        .consts
        .iter()
        .map(|c| json!({ "name": c.name, "value": fmt_const_value(&c.value), "used": c.used }))
        .collect();

    let sections: Vec<Value> = map
        .sections
        .iter()
        .map(|s| {
            json!({
                "name":        s.name,
                "address":     format!("0x{:016x}", s.abs_start),
                "offset":      format!("0x{:016x}", s.off),
                "file_offset": format!("0x{:016x}", s.file_offset),
                "size":        s.size,
            })
        })
        .collect();

    let labels: Vec<Value> = map
        .labels
        .iter()
        .map(|l| {
            json!({
                "name":        l.name,
                "address":     format!("0x{:016x}", l.abs_addr),
                "offset":      format!("0x{:016x}", l.off),
                "file_offset": format!("0x{:016x}", l.file_offset),
            })
        })
        .collect();

    let root = json!({
        "output_file": map.output_file,
        "base_addr":   format!("0x{:016x}", map.base_addr),
        "total_size":  map.total_size,
        "constants":   constants,
        "sections":    sections,
        "labels":      labels,
    });

    serde_json::to_string_pretty(&root).expect("JSON serialization failed")
}

// -- Rust formatter ------------------------------------------------------------

/// Produces a Rust module format output containing static address mappings natively.
pub fn format_rs(map: &MapDb) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    let stem = std::path::Path::new(&map.output_file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("OUTPUT")
        .replace(|c: char| !c.is_ascii_alphanumeric(), "_");

    writeln!(out, "// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!").unwrap();
    writeln!(out, "// Automatically generated file! Do not edit!").unwrap();
    writeln!(out, "// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!").unwrap();
    writeln!(out, "pub mod {}_map {{", stem).unwrap();
    writeln!(out, "    #![allow(dead_code)]\n").unwrap(); // Prevent compilation warnings if endpoints are unused

    writeln!(
        out,
        "    pub const BASE_ADDR: u64 = 0x{:016x};",
        map.base_addr
    )
    .unwrap();
    writeln!(out, "    pub const TOTAL_SIZE: u64 = {};", map.total_size).unwrap();

    if !map.consts.is_empty() {
        writeln!(out, "\n    // Constants").unwrap();
        for c in &map.consts {
            let rs_val = match &c.value {
                ParameterValue::U64(v) => format!("0x{:016x}", v),
                ParameterValue::I64(v) => format!("{}", v),
                ParameterValue::Integer(v) => format!("{}", v),
                ParameterValue::QuotedString(s) => format!("\"{}\"", s),
                _ => continue,
            };

            let rs_type = match &c.value {
                ParameterValue::U64(_) => "u64",
                ParameterValue::I64(_) => "i64",
                ParameterValue::Integer(_) => "i64", // default generic integers to signed equivalent to standard C mappings unless explicit
                ParameterValue::QuotedString(_) => "&str",
                _ => continue,
            };

            let name = c
                .name
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                .to_uppercase();
            writeln!(out, "    pub const {}: {} = {};", name, rs_type, rs_val).unwrap();
        }
    }

    if !map.sections.is_empty() {
        writeln!(out, "\n    // Sections").unwrap();
        for sec in &map.sections {
            let sec_name = sec
                .name
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                .to_uppercase();
            writeln!(
                out,
                "    pub const {}_ADDR: u64 = 0x{:016x};",
                sec_name, sec.abs_start
            )
            .unwrap();
            writeln!(
                out,
                "    pub const {}_OFFSET: u64 = 0x{:016x};",
                sec_name, sec.off
            )
            .unwrap();
            writeln!(
                out,
                "    pub const {}_FILE_OFFSET: u64 = 0x{:016x};",
                sec_name, sec.file_offset
            )
            .unwrap();
            writeln!(out, "    pub const {}_SIZE: u64 = {};", sec_name, sec.size).unwrap();
            writeln!(out).unwrap();
        }
    }

    if !map.labels.is_empty() {
        writeln!(out, "    // Labels").unwrap();
        for lab in &map.labels {
            let lab_name = lab
                .name
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                .to_uppercase();
            writeln!(
                out,
                "    pub const {}_ADDR: u64 = 0x{:016x};",
                lab_name, lab.abs_addr
            )
            .unwrap();
            writeln!(
                out,
                "    pub const {}_OFFSET: u64 = 0x{:016x};",
                lab_name, lab.off
            )
            .unwrap();
            writeln!(
                out,
                "    pub const {}_FILE_OFFSET: u64 = 0x{:016x};",
                lab_name, lab.file_offset
            )
            .unwrap();
            writeln!(out).unwrap();
        }
    }

    writeln!(out, "}}").unwrap();
    out
}

// -- MapDb construction --------------------------------------------------------

pub fn build(
    location_db: &LocationDb,
    irdb: &IRDb,
    output_file: &str,
    _diags: &mut Diags,
) -> MapDb {
    let mut sections: Vec<SectionEntry> = Vec::new();
    let mut labels: Vec<LabelEntry> = Vec::new();

    let mut stack: Vec<(String, usize)> = Vec::new();
    for (ir_index, ir) in irdb.ir_vec.iter().enumerate() {
        match ir.kind {
            IRKind::SectionStart => {
                let name = irdb.get_opnd_as_identifier(ir, 0).to_string();
                stack.push((name, ir_index));
            }
            IRKind::SectionEnd => {
                let (name, start_ir) = stack.pop().unwrap();
                let file_start = location_db.ir_locs[start_ir].file_offset;
                let file_end = location_db.ir_locs[ir_index].file_offset;

                let content_idx = ((start_ir + 1)..ir_index)
                    .find(|&j| irdb.ir_vec[j].kind != IRKind::SetAddr)
                    .unwrap_or(ir_index);
                let content_loc = &location_db.ir_locs[content_idx];

                sections.push(SectionEntry {
                    name,
                    file_offset: file_start,
                    off: content_loc.addr.addr_offset,
                    abs_start: content_loc.addr.addr_base + content_loc.addr.addr_offset,
                    size: file_end - file_start,
                });
            }
            IRKind::Label => {
                let name = irdb.get_opnd_as_identifier(ir, 0).to_string();
                let loc = &location_db.ir_locs[ir_index];
                labels.push(LabelEntry {
                    name,
                    file_offset: loc.file_offset,
                    off: loc.addr.addr_offset,
                    abs_addr: loc.addr.addr_base + loc.addr.addr_offset,
                });
            }
            _ => {}
        }
    }

    let mut consts: Vec<ConstEntry> = irdb
        .symbol_table
        .iter_defined_with_used()
        .map(|(name, pv, used)| ConstEntry {
            name: name.to_string(),
            value: pv.clone(),
            used,
        })
        .collect();
    consts.sort_by(|a, b| a.name.cmp(&b.name));

    let base_addr = sections.first().map(|s| s.abs_start).unwrap_or(0);
    let total_size = sections.last().map(|s| s.file_offset + s.size).unwrap_or(0);

    MapDb {
        output_file: output_file.to_string(),
        base_addr,
        total_size,
        sections,
        labels,
        consts,
    }
}
// -- Unit tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ParameterValue;

    fn make_map() -> MapDb {
        MapDb {
            output_file: "out.bin".to_string(),
            base_addr: 0x1000,
            total_size: 0x80,
            sections: vec![
                SectionEntry {
                    name: "text".to_string(),
                    file_offset: 0x00,
                    off: 0x00,
                    abs_start: 0x1000,
                    size: 0x40,
                },
                SectionEntry {
                    name: "data".to_string(),
                    file_offset: 0x40,
                    off: 0x40,
                    abs_start: 0x1040,
                    size: 0x40,
                },
            ],
            labels: vec![
                LabelEntry {
                    name: "start".to_string(),
                    file_offset: 0x00,
                    off: 0x00,
                    abs_addr: 0x1000,
                },
                LabelEntry {
                    name: "end_marker".to_string(),
                    file_offset: 0x7f,
                    off: 0x7f,
                    abs_addr: 0x107f,
                },
            ],
            consts: vec![
                ConstEntry {
                    name: "BASE".to_string(),
                    value: ParameterValue::U64(0x1000),
                    used: true,
                },
                ConstEntry {
                    name: "COUNT".to_string(),
                    value: ParameterValue::Integer(42),
                    used: true,
                },
                ConstEntry {
                    name: "VERSION".to_string(),
                    value: ParameterValue::QuotedString("v1.0".to_string()),
                    used: true,
                },
                ConstEntry {
                    name: "OFFSET".to_string(),
                    value: ParameterValue::I64(-10),
                    used: true,
                },
            ],
        }
    }

    #[test]
    fn header_contains_output_file_and_base_addr() {
        let out = format_csv(&make_map());
        assert!(out.contains("out.bin"), "output file name missing");
        assert!(out.contains("0x0000000000001000"), "base addr missing");
    }

    #[test]
    fn header_contains_total_size() {
        let out = format_csv(&make_map());
        assert!(out.contains("128"), "total size in bytes missing");
    }

    #[test]
    fn sections_contain_names_and_addresses() {
        let out = format_csv(&make_map());
        assert!(out.contains("text"), "section name 'text' missing");
        assert!(out.contains("data"), "section name 'data' missing");
        // abs_start of 'text' section
        assert!(
            out.contains("0x0000000000001000"),
            "'text' abs_start missing"
        );
        // abs_start of 'data' section
        assert!(
            out.contains("0x0000000000001040"),
            "'data' abs_start missing"
        );
    }

    #[test]
    fn sections_contain_sizes() {
        let out = format_csv(&make_map());
        assert!(out.contains("64,"), "section size '64' missing");
    }

    #[test]
    fn labels_contain_names_and_addresses() {
        let out = format_csv(&make_map());
        assert!(out.contains("start"), "label 'start' missing");
        assert!(out.contains("end_marker"), "label 'end_marker' missing");
        assert!(
            out.contains("0x000000000000107f"),
            "label 'end_marker' abs_addr missing"
        );
    }

    #[test]
    fn consts_appear_before_sections_in_output() {
        let out = format_csv(&make_map());
        let const_pos = out.find("Constants").expect("Constants section missing");
        let section_pos = out.find("Sections").expect("Sections section missing");
        assert!(
            const_pos < section_pos,
            "Constants must appear before Sections"
        );
    }

    #[test]
    fn consts_contain_names_and_values() {
        let out = format_csv(&make_map());
        assert!(out.contains("BASE"), "const name 'BASE' missing");
        assert!(out.contains("COUNT"), "const name 'COUNT' missing");
        assert!(out.contains("OFFSET"), "const name 'OFFSET' missing");
        assert!(out.contains("VERSION"), "const name 'VERSION' missing");
        // U64 renders as hex
        assert!(
            out.contains("0x0000000000001000"),
            "const U64 hex value missing"
        );
        // Integer renders as decimal
        assert!(out.contains("42"), "const Integer decimal value missing");
        // I64 renders as negative decimal string
        assert!(out.contains("-10"), "const I64 neg value missing");
        // String renders inside quotes
        assert!(out.contains("\"v1.0\""), "const QuotedString value missing");
        // used consts show "yes"
        assert!(out.contains("yes"), "used column 'yes' missing");
    }

    #[test]
    fn consts_unused_shows_no() {
        let map = MapDb {
            output_file: "x.bin".to_string(),
            base_addr: 0,
            total_size: 0,
            sections: vec![],
            labels: vec![],
            consts: vec![ConstEntry {
                name: "UNUSED".to_string(),
                value: ParameterValue::U64(0),
                used: false,
            }],
        };
        let out = format_csv(&map);
        assert!(
            out.contains("no"),
            "used column 'no' missing for unused const"
        );
    }

    #[test]
    fn empty_sections_shows_none() {
        let map = MapDb {
            output_file: "x.bin".to_string(),
            base_addr: 0,
            total_size: 0,
            sections: vec![],
            labels: vec![],
            consts: vec![],
        };
        let out = format_csv(&map);
        // Each table should report (none) when empty
        assert_eq!(
            out.matches("(none)").count(),
            3,
            "expected (none) for each empty table"
        );
    }

    #[test]
    fn repeated_section_name_appears_multiple_times() {
        let map = MapDb {
            output_file: "y.bin".to_string(),
            base_addr: 0,
            total_size: 0x20,
            sections: vec![
                SectionEntry {
                    name: "foo".to_string(),
                    file_offset: 0x00,
                    off: 0x00,
                    abs_start: 0x00,
                    size: 0x10,
                },
                SectionEntry {
                    name: "foo".to_string(),
                    file_offset: 0x10,
                    off: 0x10,
                    abs_start: 0x10,
                    size: 0x10,
                },
            ],
            labels: vec![],
            consts: vec![],
        };
        let out = format_csv(&map);
        assert_eq!(
            out.matches("foo").count(),
            2,
            "repeated section 'foo' should appear twice"
        );
    }

    #[test]
    fn fmt_const_value_variants() {
        assert_eq!(
            fmt_const_value(&ParameterValue::U64(0x10)),
            "0x0000000000000010"
        );
        assert_eq!(fmt_const_value(&ParameterValue::I64(-7)), "-7");
        assert_eq!(fmt_const_value(&ParameterValue::Integer(99)), "99");
        assert_eq!(
            fmt_const_value(&ParameterValue::QuotedString("hi".to_string())),
            "\"hi\""
        );
    }

    // -- format_json tests -----------------------------------------------------

    #[test]
    fn json_is_valid_and_contains_output_file() {
        let out = format_json(&make_map());
        let v: serde_json::Value = serde_json::from_str(&out).expect("output is not valid JSON");
        assert_eq!(v["output_file"], "out.bin");
    }

    #[test]
    fn json_header_fields() {
        let out = format_json(&make_map());
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["base_addr"], "0x0000000000001000");
        assert_eq!(v["total_size"], 0x80u64);
    }

    #[test]
    fn json_sections_contain_names_and_addresses() {
        let out = format_json(&make_map());
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let sections = v["sections"].as_array().unwrap();
        let names: Vec<&str> = sections
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"text"), "section 'text' missing");
        assert!(names.contains(&"data"), "section 'data' missing");
        // text abs_start = 0x1000, off = 0x00, file_offset = 0x00
        let text = sections.iter().find(|s| s["name"] == "text").unwrap();
        assert_eq!(text["address"], "0x0000000000001000");
        assert_eq!(text["size"], 0x40u64);
    }

    #[test]
    fn json_labels_contain_names_and_addresses() {
        let out = format_json(&make_map());
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let labels = v["labels"].as_array().unwrap();
        let start = labels.iter().find(|l| l["name"] == "start").unwrap();
        assert_eq!(start["address"], "0x0000000000001000");
        let end_marker = labels.iter().find(|l| l["name"] == "end_marker").unwrap();
        assert_eq!(end_marker["address"], "0x000000000000107f");
    }

    #[test]
    fn json_consts_contain_names_and_values() {
        let out = format_json(&make_map());
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let consts = v["constants"].as_array().unwrap();
        let base = consts.iter().find(|c| c["name"] == "BASE").unwrap();
        assert_eq!(base["value"], "0x0000000000001000");
        assert_eq!(base["used"], true);
        let count = consts.iter().find(|c| c["name"] == "COUNT").unwrap();
        assert_eq!(count["value"], "42");
        assert_eq!(count["used"], true);
        let offset = consts.iter().find(|c| c["name"] == "OFFSET").unwrap();
        assert_eq!(offset["value"], "-10");
        let version = consts.iter().find(|c| c["name"] == "VERSION").unwrap();
        assert_eq!(version["value"], "\"v1.0\"");
    }

    #[test]
    fn json_empty_tables_are_empty_arrays() {
        let map = MapDb {
            output_file: "x.bin".to_string(),
            base_addr: 0,
            total_size: 0,
            sections: vec![],
            labels: vec![],
            consts: vec![],
        };
        let out = format_json(&map);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["sections"].as_array().unwrap().len(), 0);
        assert_eq!(v["labels"].as_array().unwrap().len(), 0);
        assert_eq!(v["constants"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn json_repeated_section_produces_multiple_entries() {
        let map = MapDb {
            output_file: "y.bin".to_string(),
            base_addr: 0,
            total_size: 0x20,
            sections: vec![
                SectionEntry {
                    name: "foo".to_string(),
                    file_offset: 0x00,
                    off: 0x00,
                    abs_start: 0x00,
                    size: 0x10,
                },
                SectionEntry {
                    name: "foo".to_string(),
                    file_offset: 0x10,
                    off: 0x10,
                    abs_start: 0x10,
                    size: 0x10,
                },
            ],
            labels: vec![],
            consts: vec![],
        };
        let out = format_json(&map);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["sections"].as_array().unwrap().len(), 2);
    }

    // -- format_rs tests -------------------------------------------------------

    #[test]
    fn rs_is_valid_and_contains_output_file() {
        let out = format_rs(&make_map());
        assert!(out.contains("pub mod out_map {"));
        assert!(out.contains("pub const BASE_ADDR: u64 = 0x0000000000001000;"));
    }

    #[test]
    fn rs_sections_contain_names_and_addresses() {
        let out = format_rs(&make_map());
        assert!(out.contains("pub const TEXT_ADDR: u64 = 0x0000000000001000;"));
        assert!(out.contains("pub const TEXT_SIZE: u64 = 64;"));
        assert!(out.contains("pub const DATA_ADDR: u64 = 0x0000000000001040;"));
        assert!(out.contains("pub const DATA_SIZE: u64 = 64;"));
    }

    #[test]
    fn rs_labels_contain_names_and_addresses() {
        let out = format_rs(&make_map());
        assert!(out.contains("pub const START_ADDR: u64 = 0x0000000000001000;"));
        assert!(out.contains("pub const END_MARKER_ADDR: u64 = 0x000000000000107f;"));
    }

    #[test]
    fn rs_consts_contain_names_and_values() {
        let out = format_rs(&make_map());
        assert!(out.contains("pub const BASE: u64 = 0x0000000000001000;"));
        assert!(out.contains("pub const COUNT: i64 = 42;"));
        assert!(out.contains("pub const OFFSET: i64 = -10;"));
        assert!(out.contains("pub const VERSION: &str = \"v1.0\";"));
    }
}
