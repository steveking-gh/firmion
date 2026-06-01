// Binary generation and extension execution.
//
// ExecPhase forms the final stage of the compiler pipeline.  ExecPhase consumes
// LocationDb and MapDb to construct the output binary file.  Core operations
// include writing inline data, padding bytes, and referenced file contents.
//
// ExecPhase invokes compiler extensions.  Pass 1 builds the image in an
// OutputBuffer, pre-filling each extension slot with zeros.  Pass 2 executes
// extensions, reading section slices from the buffer and patching their output
// back in place.  The completed buffer is written to disk once at the end.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

pub mod output_buffer;
pub mod validation_phase;
use anyhow::{Result, anyhow};
use diags::{Diags, SourceSpan};
use firmion_extension::extension_registry::{ExtensionRegistry, ParamArg, ParamKind};
use ir::{DataType, IR, IRKind, ParameterValue};
use irdb::IRDb;
use layout_phase::locationdb::LocationDb;
use layout_phase::mapdb::MapDb;
use layout_phase::packed_size;
use const_eval::ireval::{ParmValDb, evaluate_string_expr, execute_assert};
use crate::output_buffer::OutputBuffer;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Seek, SeekFrom};

#[allow(unused_imports)]
use tracing::{debug, error, info, trace, warn};

type WrittenRanges = BTreeMap<u64, (u64, SourceSpan)>;



pub struct ExecPhase {}

impl ExecPhase {
    pub fn execute(
        location_db: &LocationDb,
        argvaldb: &ParmValDb,
        map_db: &MapDb,
        irdb: &IRDb,
        diags: &mut Diags,
        file: &mut File,
        ext_registry: &ExtensionRegistry,
    ) -> Result<()> {
        trace!("Engine::execute:");
        let mut written_ranges = BTreeMap::new();
        let mut output = OutputBuffer::new();

        Self::execute_core_operations(
            location_db,
            argvaldb,
            &mut written_ranges,
            irdb,
            diags,
            &mut output,
            ext_registry,
        )?;
        Self::execute_extensions(
            location_db,
            argvaldb,
            map_db,
            irdb,
            diags,
            &mut output,
            ext_registry,
        )?;

        output.write_to_file(file).map_err(|e| anyhow!("Failed to write output file: {}", e))?;
        Self::execute_post_output(argvaldb, irdb, diags)
    }

    // Execute print and assert statements that follow the output sentinel.
    fn execute_post_output(
        argvaldb: &ParmValDb,
        irdb: &IRDb,
        diags: &mut Diags,
    ) -> Result<()> {
        let mut past_sentinel = false;
        for ir in &irdb.ir_vec {
            if ir.kind == IRKind::Output {
                past_sentinel = true;
                continue;
            }
            if !past_sentinel {
                continue;
            }
            match ir.kind {
                IRKind::Print if !diags.noprint => {
                    if let Some(s) = evaluate_string_expr(
                        &argvaldb.parms,
                        &irdb.parms,
                        &ir.operands,
                        diags,
                    ) {
                        print!("{}", s);
                    }
                }
                IRKind::Trace if diags.trace_enabled() => {
                    if let Some(mut s) = evaluate_string_expr(
                        &argvaldb.parms,
                        &irdb.parms,
                        &ir.operands,
                        diags,
                    ) {
                        let prefix = format!("[Trace-{}] ", diags.trace_iteration);
                        s.insert_str(0, &prefix);
                        print!("{}", s);
                    }
                }
                IRKind::Assert
                    if !execute_assert(&argvaldb.parms, &irdb.parms, &irdb.ir_vec, ir, diags) =>
                {
                    return Err(anyhow!("Assert failed"));
                }
                _ => {}
            }
        }
        Ok(())
    }


    #[allow(clippy::too_many_arguments)]
    fn execute_wrs(
        location_db: &LocationDb,
        argvaldb: &ParmValDb,
        written_ranges: &mut WrittenRanges,
        lid: usize,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        output: &mut OutputBuffer,
    ) -> Result<()> {
        trace!("Engine::execute_wrs:");
        let Some(xstr) = evaluate_string_expr(&argvaldb.parms, &irdb.parms, &ir.operands, diags)
        else {
            let msg = "Evaluating string expression failed.".to_string();
            diags.err1("ERR_139", &msg, ir.src_loc.clone());
            return Err(anyhow!("Wrs failed"));
        };
        let size = xstr.len() as u64;
        let loc = &location_db.ir_locs[lid];
        let addr = loc.addr.addr_base + loc.addr.addr_offset;
        if !Self::check_and_record_range(written_ranges, addr, size, ir.src_loc.clone(), diags) {
            return Err(anyhow!("Address overwrite detected"));
        }

        output.append(xstr.as_bytes());
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_wrf(
        location_db: &LocationDb,
        argvaldb: &ParmValDb,
        written_ranges: &mut WrittenRanges,
        lid: usize,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        output: &mut OutputBuffer,
    ) -> Result<()> {
        trace!("Engine::execute_wrf:");

        let path = argvaldb.parms[ir.operands[0]].to_str().to_owned();

        // IRDb pre-validated this path; unwrap is safe.
        let file_size = irdb.files.get(path.as_str()).unwrap().size;

        let loc = &location_db.ir_locs[lid];
        let addr = loc.addr.addr_base + loc.addr.addr_offset;
        if !Self::check_and_record_range(written_ranges, addr, file_size, ir.src_loc.clone(), diags)
        {
            return Err(anyhow!("Address overwrite detected"));
        }

        let mut source_file = match File::open(path.as_str()) {
            Ok(f) => f,
            Err(err) => {
                let msg = format!(
                    "Opening file '{path}' failed with OS error '{:?}'.",
                    err.raw_os_error()
                );
                diags.err1("ERR_155", &msg, ir.src_loc.clone());
                return Err(anyhow!(err));
            }
        };

        if let Err(err) = output.append_from_file(&mut source_file, file_size) {
            let msg = format!(
                "Reading file '{path}' failed with OS error '{:?}'.",
                err.raw_os_error()
            );
            diags.err1("ERR_156", &msg, ir.src_loc.clone());
            return Err(anyhow!(err));
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_wrobj(
        location_db: &LocationDb,
        argvaldb: &ParmValDb,
        written_ranges: &mut WrittenRanges,
        lid: usize,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        output: &mut OutputBuffer,
    ) -> Result<()> {
        trace!("Engine::execute_wrobj:");

        let obj_name = argvaldb.parms[ir.operands[0]].identifier_to_str().to_owned();

        // IRDb pre-validated this entry; unwrap is safe.
        let infos = irdb.objsecs.get(&obj_name).unwrap();

        let loc = &location_db.ir_locs[lid];
        let addr = loc.addr.addr_base + loc.addr.addr_offset;
        if !Self::check_and_record_range(
            written_ranges,
            addr,
            packed_size(infos),
            ir.src_loc.clone(),
            diags,
        ) {
            return Err(anyhow!("Address overwrite detected"));
        }

        // Write each section in order, inserting zero-fill alignment padding.
        let mut write_offset: u64 = 0;
        for info in infos {
            let align = info.align.max(1);
            let aligned = (write_offset + align - 1) & !(align - 1);
            let pad = (aligned - write_offset) as usize;
            if pad > 0 {
                output.append_zeros(pad);
            }
            write_offset = aligned;

            let file_path = &info.file;
            let mut source_file = match File::open(file_path.as_str()) {
                Ok(f) => f,
                Err(err) => {
                    let msg = format!(
                        "Opening object file '{file_path}' failed with OS error '{:?}'.",
                        err.raw_os_error()
                    );
                    diags.err1("ERR_193", &msg, ir.src_loc.clone());
                    return Err(anyhow!(err));
                }
            };

            if let Err(err) = source_file.seek(SeekFrom::Start(info.file_offset)) {
                let msg = format!(
                    "Seeking in object file '{file_path}' failed with OS error '{:?}'.",
                    err.raw_os_error()
                );
                diags.err1("ERR_194", &msg, ir.src_loc.clone());
                return Err(anyhow!(err));
            }

            if let Err(err) = output.append_from_file(&mut source_file, info.size) {
                let msg = format!(
                    "Reading object file '{file_path}' failed with OS error '{:?}'.",
                    err.raw_os_error()
                );
                diags.err1("ERR_195", &msg, ir.src_loc.clone());
                return Err(anyhow!(err));
            }

            write_offset += info.size;
        }

        Ok(())
    }

    fn execute_wrx(
        location_db: &LocationDb,
        argvaldb: &ParmValDb,
        written_ranges: &mut WrittenRanges,
        lid: usize,
        ir: &IR,
        diags: &mut Diags,
        output: &mut OutputBuffer,
    ) -> Result<()> {
        trace!("{}", format!("Engine::execute_wrx: {:?}", ir.kind).as_str());
        let byte_size = ir.get_wrx_byte_width();

        let opnd_num = ir.operands[0];
        trace!(
            "{}",
            format!("engine::execute_wrx: checking operand {}", opnd_num).as_str()
        );
        let parm = &argvaldb.parms[opnd_num];

        let big_endian = matches!(ir.kind, IRKind::Wr(_, true));

        // Extract 8 bytes in the requested byte order, then slice to byte_size.
        // For little-endian the LSB is at index 0; for big-endian the MSB is at
        // index 0 and the desired bytes are the trailing byte_size bytes.
        let buf = match parm.data_type() {
            DataType::Integer | DataType::I64 => {
                let val = parm.to_i64();
                if big_endian { val.to_be_bytes() } else { val.to_le_bytes() }
            }
            DataType::U64 => {
                let val = parm.to_u64();
                if big_endian { val.to_be_bytes() } else { val.to_le_bytes() }
            }
            bad => {
                panic!("Unexpected parameter type {:?} in execute_wrx", bad);
            }
        };

        let mut repeat_count = 1;

        if ir.operands.len() == 2 {
            // Yes, we have a repeat count
            // We already validated the operands in IRDB.
            let repeat_opnd_num = ir.operands[1];
            let op = &argvaldb.parms[repeat_opnd_num];
            repeat_count = op.to_u64();
        }

        trace!("{}", format!("Repeat count = {}", repeat_count).as_str());
        let total_size = (byte_size as u64) * repeat_count;
        let loc = &location_db.ir_locs[lid];
        let addr = loc.addr.addr_base + loc.addr.addr_offset;
        if !Self::check_and_record_range(
            written_ranges,
            addr,
            total_size,
            ir.src_loc.clone(),
            diags,
        ) {
            return Err(anyhow!("Address overwrite detected"));
        }

        // For LE, take the first byte_size bytes.  For BE, to_be_bytes() places
        // the MSB at index 0, so the significant byte_size bytes are at the end.
        let start = if big_endian { 8 - byte_size } else { 0 };
        while repeat_count > 0 {
            output.append(&buf[start..start + byte_size]);
            repeat_count -= 1;
        }

        Ok(())
    }

    fn execute_core_operations(
        location_db: &LocationDb,
        argvaldb: &ParmValDb,
        written_ranges: &mut WrittenRanges,
        irdb: &IRDb,
        diags: &mut Diags,
        output: &mut OutputBuffer,
        ext_registry: &ExtensionRegistry,
    ) -> Result<()> {
        trace!("Engine::execute_core_operations:");
        let mut result;
        let mut error_count = 0;
        for (lid, ir) in irdb.ir_vec.iter().enumerate() {
            if ir.kind == IRKind::Output {
                break;
            }
            result = match ir.kind {
                IRKind::Wr(_, _) => Self::execute_wrx(location_db, argvaldb, written_ranges, lid, ir, diags, output),
                // Pre-output prints and traces already fired in validation_phase.
                IRKind::Print | IRKind::Trace => Ok(()),
                IRKind::Wrs => Self::execute_wrs(location_db, argvaldb, written_ranges, lid, ir, irdb, diags, output),
                IRKind::Wrf => Self::execute_wrf(location_db, argvaldb, written_ranges, lid, ir, irdb, diags, output),
                IRKind::Wrobj => Self::execute_wrobj(location_db, argvaldb, written_ranges, lid, ir, irdb, diags, output),
                // Assert evaluates in the validation phase, before byte generation.
                IRKind::Assert => Ok(()),
                // the rest of these operations are computed during iteration
                IRKind::SetSecOffset
                | IRKind::SetAddrOffset
                | IRKind::SetAddr
                | IRKind::SetFileOffset
                | IRKind::Align
                | IRKind::Addr
                | IRKind::Const
                | IRKind::AddrOffset
                | IRKind::SecOffset
                | IRKind::FileOffset
                | IRKind::Label
                | IRKind::ObjAlign
                | IRKind::ObjLma
                | IRKind::ObjVma
                | IRKind::Sizeof
                | IRKind::SizeofExt
                | IRKind::BuiltinOutputSize
                | IRKind::BuiltinOutputAddr
                | IRKind::BuiltinVersionString
                | IRKind::BuiltinVersionMajor
                | IRKind::BuiltinVersionMinor
                | IRKind::BuiltinVersionPatch
                | IRKind::ToI64
                | IRKind::ToU64
                | IRKind::Eq
                | IRKind::NEq
                | IRKind::GEq
                | IRKind::LEq
                | IRKind::Gt
                | IRKind::Lt
                | IRKind::DoubleEq
                | IRKind::I64
                | IRKind::U64
                | IRKind::BitAnd
                | IRKind::LogicalAnd
                | IRKind::BitOr
                | IRKind::LogicalOr
                | IRKind::Multiply
                | IRKind::Modulo
                | IRKind::Divide
                | IRKind::Add
                | IRKind::Subtract
                | IRKind::SectionStart
                | IRKind::SectionEnd
                | IRKind::LeftShift
                | IRKind::RightShift
                // if/else IR only lives in const_ir_vec; never reaches the engine.
                | IRKind::ConstDeclare
                | IRKind::IfBegin
                | IRKind::ElseBegin
                | IRKind::IfEnd
                | IRKind::BareAssign => Ok(()),
                // Handled by the early break above; the match arm is required for exhaustiveness.
                IRKind::Output => unreachable!(),
                IRKind::ExtensionCall => {
                    // Reserve zeroed bytes for the extension output slot.
                    // Pass 2 patches the actual output back into this region.
                    let ext_name = argvaldb.parms[ir.operands[0]].identifier_to_str();
                    if let Some(entry) = ext_registry.get(ext_name) {
                        output.append_zeros(entry.cached_size);
                    }
                    Ok(())
                }
            };

            if result.is_err() {
                error_count += 1;
                if error_count > 10 {
                    // todo parameterize max 10 errors
                    break;
                }
            }
        }

        if error_count > 0 {
            return Err(anyhow!("Error detected"));
        }
        Ok(())
    }

    fn execute_extensions(
        location_db: &LocationDb,
        argvaldb: &ParmValDb,
        map_db: &MapDb,
        irdb: &IRDb,
        diags: &mut Diags,
        output: &mut OutputBuffer,
        ext_registry: &ExtensionRegistry,
    ) -> Result<()> {
        trace!("Engine::execute_extensions:");

        let mut extension_nodes = Vec::new();
        for (idx, ir) in irdb.ir_vec.iter().enumerate() {
            if ir.kind == IRKind::ExtensionCall {
                extension_nodes.push(idx);
            }
        }

        if extension_nodes.is_empty() {
            return Ok(());
        }

        // Maps each section name to a list of indices into wr_dispatches.
        // Slice param resolution uses the map to check for ambiguity and to
        // resolve file_offset in O(1) instead of two linear scans per call.
        let mut sec_dispatch_map: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, d) in map_db.sections.iter().enumerate() {
            sec_dispatch_map.entry(d.name.as_str()).or_default().push(i);
        }

        // Sequentially execute extensions and patch the output file.
        let mut error_count = 0;
        for &idx in &extension_nodes {
            let ir = &irdb.ir_vec[idx];
            let name = argvaldb.parms[ir.operands[0]].identifier_to_str();

            let Some(entry) = ext_registry.get(name) else {
                unreachable!("Extension '{}' not found in registry", name);
            };
            let byte_width = entry.cached_size;

            // Build the ParamArg list and call the extension.
            //
            // All extension calls use IRKind::ExtensionCall with operand layout:
            //   [name, user_arg0..., output]
            //
            // The trailing output operand is a type-checking placeholder and
            // must not be passed to the extension.  last = len-1 excludes it.
            //
            // The engine resolves Slice params to ParamArg::Slice and passes
            // remaining params as ParamArg::Int or ParamArg::Str.  Operands arrive in
            // declaration order (irdb canonicalized them).
            //
            // ParamArg::Slice holds &mmap[..], an immutable borrow.  Pre-resolve
            // all section lookups before that scope so error handling can use `continue`.
            let cached_params = &entry.cached_params;

            // Per-param section resolutions, indexed parallel to cached_params.
            // Each entry is Some((file_offset, size, slice_start, slice_end)) for
            // Slice params, or None for Int/Str params.
            let mut resolved_sections: Vec<Option<(u64, u64, usize, usize)>> = Vec::new();
            let mut section_resolve_failed = false;

            // Resolve each Slice param.
            // Operands arrive in declaration order after irdb canonicalization.
            for (i, p) in cached_params.iter().enumerate() {
                if p.kind == ParamKind::Slice {
                    let sec_name = argvaldb.parms[ir.operands[1 + i]]
                        .identifier_to_str()
                        .to_string();
                    let indices = sec_dispatch_map
                        .get(sec_name.as_str())
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    if indices.len() > 1 {
                        diags.err1(
                            "ERR_173",
                            &format!(
                                "Extension '{}': section '{}' for parameter '{}' appears {} \
                                 times in the output and is ambiguous.",
                                name,
                                sec_name,
                                p.name,
                                indices.len(),
                            ),
                            ir.src_loc.clone(),
                        );
                        error_count += 1;
                        section_resolve_failed = true;
                        break;
                    }
                    let Some(&di) = indices.first() else {
                        return Err(anyhow!(
                            "Extension '{}': section '{}' for parameter '{}' not found \
                             in dispatch table. This is a compiler bug.",
                            name,
                            sec_name,
                            p.name
                        ));
                    };
                    let d = &map_db.sections[di];
                    let start = d.file_offset as usize;
                    let end = start + d.size as usize;
                    if end > output.len() || start > end {
                        return Err(anyhow!(
                            "Extension '{}' slice read for parameter '{}' (range {:#x}..{:#x}) exceeds buffer bounds (length {}). This is a severe compiler bug.",
                            name,
                            p.name,
                            start,
                            end,
                            output.len()
                        ));
                    }
                    resolved_sections.push(Some((
                        d.file_offset,
                        d.size,
                        start,
                        end,
                    )));
                } else {
                    resolved_sections.push(None);
                }
            }

            if section_resolve_failed {
                continue;
            }

            // Build ext_args in a scope that isolates the immutable mmap borrow
            // held by ParamArg::Slice.  The scope produces only an owned Vec<u8>,
            // so the borrow drops before the mutable patch write below.
            let exec_result: Result<Vec<u8>, String> = {
                let mut ext_args: Vec<ParamArg<'_>> = Vec::new();

                for (i, p) in cached_params.iter().enumerate() {
                    if p.kind == ParamKind::Slice {
                        if let Some((_file_offset, _len, start, end)) = resolved_sections[i] {
                            ext_args.push(ParamArg::Slice {
                                data: output.slice(start, end),
                            });
                        }
                    } else {
                        let parm = &argvaldb.parms[ir.operands[1 + i]];
                        let arg = match parm {
                            ParameterValue::U64(v) => ParamArg::Int(*v),
                            ParameterValue::I64(v) | ParameterValue::Integer(v) => {
                                ParamArg::Int(*v as u64)
                            }
                            ParameterValue::QuotedString(s) => ParamArg::Str(s.as_str()),
                            _ => unreachable!(
                                "unexpected extension arg type {:?}",
                                parm.data_type()
                            ),
                        };
                        ext_args.push(arg);
                    }
                }

                let mut out = vec![0u8; byte_width];
                entry.extension.execute(&ext_args, &mut out).map(|_| out)
            };

            let out_buffer = match exec_result {
                Ok(buf) => buf,
                Err(e) => {
                    let msg = format!("Extension '{}' execution failed: {}", name, e);
                    diags.err1("ERR_167", &msg, ir.src_loc.clone());
                    return Err(anyhow!(msg));
                }
            };

            // Patch the file at the exact image offset of this extension call.
            // ir_locs[idx] holds the file offset before this IR executes, which
            // is the start of the zeroed placeholder written during generate.
            let loc = &location_db.ir_locs[idx];
            let abs_offset = loc.file_offset as usize;

            if abs_offset + byte_width > output.len() {
                return Err(anyhow!(
                    "Extension bounded write exceeds buffer bounds. This is a severe compiler bug."
                ));
            }

            output.patch(abs_offset, &out_buffer);
        }

        if error_count > 0 {
            return Err(anyhow!("Error detected in extension execution"));
        }

        Ok(())
    }

    fn check_and_record_range(
        written_ranges: &mut WrittenRanges,
        start: u64,
        size: u64,
        src_loc: SourceSpan,
        diags: &mut Diags,
    ) -> bool {
        if size == 0 {
            return true;
        }
        let end = start + size; // safe: callers already checked for overflow

        // Check against the range that starts at or just before `start`.
        let overlap = written_ranges
            .range(..=start)
            .next_back()
            .filter(|(_, (prev_end, _))| *prev_end > start)
            .map(|(&prev_start, (prev_end, prev_loc))| (prev_start, *prev_end, prev_loc.clone()))
            .or_else(|| {
                // Also check the range that starts strictly after `start` ???
                // it overlaps if its start is less than our end.
                written_ranges
                    .range(start + 1..)
                    .next()
                    .filter(|&(&next_start, _)| next_start < end)
                    .map(|(&next_start, (next_end, next_loc))| {
                        (next_start, *next_end, next_loc.clone())
                    })
            });

        written_ranges.insert(start, (end, src_loc.clone()));

        if let Some((prev_start, prev_end, prev_loc)) = overlap {
            let msg = format!(
                "Address range {:#x}..{:#x} overlaps previously written range {:#x}..{:#x}",
                start, end, prev_start, prev_end,
            );
            diags.err2("ERR_172", &msg, src_loc, prev_loc);
            return false;
        }

        true
    }

}
