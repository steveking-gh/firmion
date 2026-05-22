// Top-level pipeline orchestrator for firmion.
//
// The process function is the single entry point that drives the entire
// compiler pipeline.  It sequences the four stages in order — Ast, LayoutDb,
// IRDb and Engine — passing each stage's output as input to the next, and
// converting any stage-level Err(()) result into an anyhow error so that the
// caller receives a descriptive failure message.  It also handles the output
// file name, creating the file before handing it to Engine for writing.
//
// Order of operations: process.rs sits above all four pipeline stages.
// main.rs calls process() once per invocation after reading the source file.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use anyhow::{Context, Result, anyhow};
use parse_int::parse;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

// Local libraries
use ast::Ast;
use ast::astdb::AstDb;
use diags::Diags;
use exec_phase::ExecPhase;
use firmion_extension::extension_registry::ExtensionRegistry;
use firmion_extension::test_mocks::register_test_extensions;
use ir::{ConstBuiltins, ParameterValue};
use irdb::IRDb;
use layout_phase::LayoutPhase;
use irdb::layoutdb::LayoutDb;
use map_phase::{format_c99, format_csv, format_json, format_rs};
use irdb::regiondb::RegionDb;
use validation_phase::ValidationPhase;

#[allow(unused_imports)]
use tracing::{debug, error, info, trace, warn};

/// Parses a single `-D` define string of the form `NAME=value` or `NAME`
/// into a `(name, ParameterValue)` pair.
///
/// Value type inference:
/// - No `=`                          → `Integer(1)` (GCC convention for bare -DFLAG)
/// - Ends with `u`                   → `U64`
/// - Ends with `i`                   → `I64`
/// - Starts with `"` / `'`          → `QuotedString` (strip surrounding quotes)
/// - Starts with `-`                 → `I64`
/// - Starts with `0x`/`0b`     `     → `U64` (matches source const behavior)
/// - Otherwise                       → `Integer`
fn parse_define(s: &str) -> Result<(String, ParameterValue)> {
    if s.is_empty() {
        return Err(anyhow!("Empty name in define '{}'", s));
    }
    let (name, val_str) = match s.find('=') {
        None => return Ok((s.to_string(), ParameterValue::Integer(1))),
        Some(pos) => (&s[..pos], &s[pos + 1..]),
    };
    if name.is_empty() {
        return Err(anyhow!("Empty name in define '{}'", s));
    }
    let value = if val_str.starts_with('"') || val_str.starts_with('\'') {
        // trim_matches only removes prefixes and suffixes, not interior chars.
        let inner = val_str.trim_matches(&['"', '\''][..]);
        ParameterValue::QuotedString(inner.to_string())
    } else if let Some(stripped) = val_str.strip_suffix('u') {
        let v =
            parse::<u64>(stripped).map_err(|e| anyhow!("Error parsing define '{}': {}", s, e))?;
        ParameterValue::U64(v)
    } else if let Some(stripped) = val_str.strip_suffix('i') {
        let v = parse::<i64>(stripped)
            .map_err(|_| anyhow!("Invalid I64 value in define '{}': '{}'", s, stripped))?;
        ParameterValue::I64(v)
    } else if val_str.starts_with('-') {
        let v = parse::<i64>(val_str)
            .map_err(|_| anyhow!("Invalid I64 value in define '{}': '{}'", s, val_str))?;
        ParameterValue::I64(v)
    } else if val_str.starts_with("0x")
        || val_str.starts_with("0X")
        || val_str.starts_with("0b")
        || val_str.starts_with("0B")
    {
        let v = parse::<u64>(val_str)
            .map_err(|_| anyhow!("Invalid U64 value in define '{}': '{}'", s, val_str))?;
        ParameterValue::U64(v)
    } else if val_str.chars().any(|c| !c.is_ascii_digit()) {
        // Non-numeric characters: treat as a bare string rather than erroring.
        // This lets -DPATH=./file.elf work without shell-level quote gymnastics.
        ParameterValue::QuotedString(val_str.to_string())
    } else {
        let v = parse::<i64>(val_str)
            .map_err(|_| anyhow!("Invalid integer value in define '{}': '{}'", s, val_str))?;
        ParameterValue::Integer(v)
    };
    Ok((name.to_string(), value))
}

/// Returns all compiled-in extension names in sorted order.
/// Excludes test-only mock extensions.
pub fn list_extensions() -> Vec<String> {
    let mut registry = ExtensionRegistry::new();
    extensions::register_all(&mut registry);
    registry
        .sorted_names()
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Entry point for all processing on the input source file.
/// This function drives the compilation pipeline.
///
/// `name`        — source file path
/// `fstr`        — source file contents
/// `output_file` — binary output path (default: "output.bin")
/// `verbosity`   — log level (0 = quiet, 1 = default, 2+ = verbose)
/// `noprint`     — suppress print statements in source
/// `defines`     — command-line const defines, e.g. `["BASE=0x1000", "COUNT=4"]`
/// `map_hf`           — human-friendly map destination: None = skip,
///                      Some("-") = stdout, Some(path) = file
/// `map_json`         — JSON map destination: None = skip,
///                      Some("-") = stdout, Some(path) = file
/// `max_output_size`  — reject images larger than this many bytes (ERR_179)
#[allow(clippy::too_many_arguments)]
pub fn process(
    name: &str,
    fstr: &str,
    output_file: Option<&str>,
    verbosity: u64,
    noprint: bool,
    defines: &[String],
    max_output_size: u64,
    map_csv: Option<&str>,
    map_json: Option<&str>,
    map_c99: Option<&str>,
    map_rs: Option<&str>,
) -> Result<()> {
    info!("Processing {}", name);
    ConstBuiltins::init();

    let mut diags = Diags::new(name, fstr, verbosity, noprint);

    // Parse -D defines into a map of pre-resolved const values.
    let mut const_defines: HashMap<String, ParameterValue> = HashMap::new();
    for d in defines {
        let (n, v) = parse_define(d)?;
        const_defines.insert(n, v);
    }

    let ast = Ast::new(name, fstr, &mut diags).context("[ERR_218]: Error detected, halting.")?;

    if verbosity > 2 {
        ast.dump("ast.dot")?;
    }

    // First AstDb: lenient (no nesting validation) — used only by const_eval.
    // Nesting validation is deferred to the post-prune AstDb below, where
    // sections promoted from top-level if/else blocks are visible.
    let ast_db = AstDb::new(&mut diags, &ast, false)?;

    let (mut symbol_table, pruned_ast) = const_eval::evaluate_and_prune(&mut diags, &ast, &ast_db, &const_defines)
        .context("[ERR_219]: Error detected, halting.")?;

    // Second AstDb: built from the pruned AST with full nesting validation.
    // Sections promoted from top-level if/else blocks are now at root level.
    let pruned_ast_db =
        AstDb::new(&mut diags, &pruned_ast, true).context("[ERR_220]: Error detected, halting.")?;

    let Some(region_bindings) =
        const_eval::evaluate_regions(&mut diags, &pruned_ast, &pruned_ast_db, &mut symbol_table)
    else {
        return Err(anyhow!("[ERR_226]: Error detected, halting."));
    };

    let Some(obj_props) =
        const_eval::evaluate_obj_props(&mut diags, &pruned_ast, &pruned_ast_db, &mut symbol_table)
    else {
        return Err(anyhow!("[ERR_232]: Error detected, halting."));
    };

    // Build the section-to-region-name map (foreign key into region_bindings).
    let mut section_region_names: HashMap<String, String> = HashMap::new();
    for (sec_name, section) in &pruned_ast_db.sections {
        if let Some(region_name) = &section.region {
            section_region_names.insert(sec_name.to_string(), region_name.clone());
        }
    }

    let layout_db = LayoutDb::new(&mut diags, &pruned_ast, &pruned_ast_db, &symbol_table, obj_props)
        .context("[ERR_221]: Error detected, halting.")?;
    if verbosity > 2 {
        layout_db.dump();
    }

    let mut ext_registry = ExtensionRegistry::new();
    register_test_extensions(&mut ext_registry);
    extensions::register_all(&mut ext_registry);

    let ir_db = IRDb::new(
        symbol_table,
        &layout_db,
        &mut diags,
        &ext_registry,
        section_region_names,
        region_bindings,
    )
    .context("[ERR_222]: Error detected, halting.")?;

    debug!("Dumping ir_db");
    if verbosity > 2 {
        ir_db.dump();
    }

    let region_db = RegionDb::build(&ir_db, &mut diags)
        .ok_or_else(|| anyhow!("[ERR_227]: Error detected, halting."))?;

    let (location_db, argval_db) =
        LayoutPhase::build(&ir_db, &region_db, &ext_registry, &mut diags)
            .context("[ERR_223]: Error detected, halting.")?;
    if verbosity > 2 {
        // LayoutPhase debug dump removed
    }

    // Check image size against --max-output-size before writing any bytes.
    // Determine if the user specified an output file on the command line
    // Trim whitespace
    let fname_str = String::from(output_file.unwrap_or("output.bin").trim_matches(' '));
    debug!("process: output file name is {}", fname_str);

    let map_db = map_phase::build(&location_db, &ir_db, &fname_str, &mut diags);
    let final_size = map_db.sections.last().map_or(0, |d| d.file_offset + d.size);
    if final_size > max_output_size {
        let msg = format!(
            "Output image size {} bytes exceeds maximum {} bytes. \
             Use --max-output-size to increase the limit.",
            final_size, max_output_size
        );
        diags.err0("ERR_224", &msg);
        return Err(anyhow!("[ERR_224]: Error detected, halting."));
    }

    ValidationPhase::validate(&argval_db, &ir_db, &mut diags)
        .context("[ERR_225]: Error detected, halting.")?;

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&fname_str)
        .context(format!("Unable to create output file {}", fname_str))?;

    if ExecPhase::execute(
        &location_db,
        &argval_db,
        &map_db,
        &ir_db,
        &mut diags,
        &mut file,
        &ext_registry,
    )
    .is_err()
    {
        return Err(anyhow!("[ERR_223]: Error detected, halting."));
    }

    // Generate map output if requested.  MapDb derives all data from the
    // post-iterate engine and irdb; no additional compiler passes run.
    if map_csv.is_some() || map_json.is_some() || map_c99.is_some() || map_rs.is_some() {
        emit_map(map_csv, &format_csv(&map_db))?;
        emit_map(map_json, &format_json(&map_db))?;
        emit_map(map_c99, &format_c99(&map_db))?;
        emit_map(map_rs, &format_rs(&map_db))?;
    }
    Ok(())
}

/// Writes `content` to stdout when `dest` is `Some("-")`, or to the named
/// file when `dest` is `Some(path)`.  Does nothing when `dest` is `None`.
fn emit_map(dest: Option<&str>, content: &str) -> Result<()> {
    match dest {
        None => {}
        Some("-") => print!("{content}"),
        Some(path) => {
            let mut f = File::create(path).context(format!("Unable to create map file {path}"))?;
            f.write_all(content.as_bytes())
                .context(format!("Unable to write map file {path}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_define;
    use ir::ParameterValue;

    fn name_val(s: &str) -> (String, ParameterValue) {
        parse_define(s).expect("parse_define failed")
    }

    // --- hex values ---

    #[test]
    fn hex_no_suffix_is_u64() {
        let (n, v) = name_val("BASE=0x1000");
        assert_eq!(n, "BASE");
        assert_eq!(v, ParameterValue::U64(0x1000));
    }

    #[test]
    fn hex_u_suffix_is_u64() {
        let (n, v) = name_val("BASE=0x1000u");
        assert_eq!(n, "BASE");
        assert_eq!(v, ParameterValue::U64(0x1000));
    }

    #[test]
    fn hex_i_suffix_is_i64() {
        let (n, v) = name_val("OFFSET=0x40i");
        assert_eq!(n, "OFFSET");
        assert_eq!(v, ParameterValue::I64(0x40));
    }

    #[test]
    fn hex_uppercase_digits() {
        let (n, v) = name_val("MASK=0xFF");
        assert_eq!(n, "MASK");
        assert_eq!(v, ParameterValue::U64(0xFF));
    }

    #[test]
    fn hex_large_u64() {
        // 0xFFFFFFFF fits in both i64 and u64; with u suffix must be U64.
        let (n, v) = name_val("LIMIT=0xFFFFFFFFu");
        assert_eq!(n, "LIMIT");
        assert_eq!(v, ParameterValue::U64(0xFFFF_FFFF));
    }

    #[test]
    fn hex_u64_max() {
        // u64::MAX requires u suffix; without it parse::<i64> would fail.
        let (n, v) = name_val("TOP=0xFFFFFFFFFFFFFFFFu");
        assert_eq!(n, "TOP");
        assert_eq!(v, ParameterValue::U64(u64::MAX));
    }

    #[test]
    fn hex_u64_max_no_suffix() {
        // 0xFFFFFFFFFFFFFFFF is valid U64 without any suffix.
        let (n, v) = name_val("TOP=0xFFFFFFFFFFFFFFFF");
        assert_eq!(n, "TOP");
        assert_eq!(v, ParameterValue::U64(u64::MAX));
    }

    // --- decimal and other cases (regression) ---

    #[test]
    fn decimal_no_suffix_is_integer() {
        let (n, v) = name_val("COUNT=42");
        assert_eq!(n, "COUNT");
        assert_eq!(v, ParameterValue::Integer(42));
    }

    #[test]
    fn decimal_u_suffix_is_u64() {
        let (n, v) = name_val("SIZE=64u");
        assert_eq!(n, "SIZE");
        assert_eq!(v, ParameterValue::U64(64));
    }

    #[test]
    fn decimal_negative_is_i64() {
        let (n, v) = name_val("SHIFT=-4");
        assert_eq!(n, "SHIFT");
        assert_eq!(v, ParameterValue::I64(-4));
    }

    #[test]
    fn bare_name_is_integer_one() {
        let (n, v) = name_val("FLAG");
        assert_eq!(n, "FLAG");
        assert_eq!(v, ParameterValue::Integer(1));
    }

    #[test]
    fn empty_name_is_error() {
        assert!(parse_define("").is_err());
        assert!(parse_define("=").is_err());
        assert!(parse_define("=42").is_err());
    }

    #[test]
    fn quoted_string_is_parsed() {
        let (n, v) = name_val("VERSION=\"1.0\"");
        assert_eq!(n, "VERSION");
        assert_eq!(v, ParameterValue::QuotedString("1.0".to_string()));

        let (n2, v2) = name_val("LABEL='stable'");
        assert_eq!(n2, "LABEL");
        assert_eq!(v2, ParameterValue::QuotedString("stable".to_string()));
    }

    #[test]
    fn bare_non_numeric_is_string() {
        // Unquoted values containing non-digit characters become bare strings.
        // This lets -DPATH=./some/file.elf work without shell-level quoting.
        let (n, v) = name_val("PATH=./.pio/build/firmware.elf");
        assert_eq!(n, "PATH");
        assert_eq!(v, ParameterValue::QuotedString("./.pio/build/firmware.elf".to_string()));

        let (n2, v2) = name_val(r"PATH=.\.pio\build\firmware.elf");
        assert_eq!(n2, "PATH");
        assert_eq!(v2, ParameterValue::QuotedString(r".\.pio\build\firmware.elf".to_string()));
    }
}
