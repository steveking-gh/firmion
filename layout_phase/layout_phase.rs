// Iterative address resolution and footprint calculation.
//
// LayoutPhase forms the fourth stage of the compiler pipeline. Code sections
// reference sizes and addresses of succeeding sections. Forward references
// prevent single-pass address resolution. LayoutPhase executes an iterative
// loop, re-evaluating every IR instruction until all location-counter values
// stabilize. Stabilization produces a LocationDb containing concrete file
// offsets and memory addresses for all operations.
//
// Order of operations: LayoutPhase executes after RegionDb and IRDb. LayoutPhase
// outputs a LocationDb for consumption by MapPhase.
//
// Region geometry (effective region intersections) is fully resolved in RegionDb
// before this phase begins and does not change across iterate passes.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

pub mod locationdb;
pub mod mapdb;
use crate::locationdb::{AddressState, Location, LocationDb};

use diags::Diags;
use firmion_extension::extension_registry::ExtensionRegistry;
use ir::{ConstBuiltins, DataType, EffectiveRegion, IR, IRKind, ObjsecInfo, ParameterValue};
use irdb::IRDb;
use const_eval::ireval::{ParmValDb, evaluate_string_expr};
use irdb::regiondb::RegionDb;
use std::collections::HashSet;

#[allow(unused_imports)]
use tracing::{debug, error, info, trace, warn};

/// Returns the total byte footprint of packing `infos` contiguously, respecting
/// each section's declared alignment.  Mirrors the byte sequence written by
/// exec_phase::execute_wrobj.
pub fn packed_size(infos: &[ObjsecInfo]) -> u64 {
    let mut offset: u64 = 0;
    for sec in infos {
        let align = sec.align.max(1);
        offset = (offset + align - 1) & !(align - 1);
        offset += sec.size;
    }
    offset
}

/// Tracks location state for one active section scope.
/// All parent-scope state saved on section entry and restored on section exit.
struct ScopeFrame {
    parent_state: AddressState,
    sec_name: String,
    set_addr_seen: bool,
    /// Pre-computed effective region from RegionDb, or None when no region
    /// constraint applies to this scope or any ancestor.
    effective_region: Option<EffectiveRegion>,
}

pub struct LayoutPhase {
    parms: Vec<ParameterValue>,
    ir_locs: Vec<Location>,

    /// One frame per active section, innermost last.  Pushed on SectionStart,
    /// popped on SectionEnd.
    scope_stack: Vec<ScopeFrame>,

    /// (lid, code) pairs for which a warning has already been emitted.
    /// Prevents duplicate diagnostics across iterate passes.
    warned_lids: HashSet<(usize, &'static str)>,
}

fn get_wrx_byte_width(ir: &IR) -> usize {
    match ir.kind {
        IRKind::Wr(w, _) => w as usize,
        bad => {
            panic!("Called get_wrx_byte_width with {:?}", bad);
        }
    }
}

impl LayoutPhase {
    /// Debug trace that produces an indented output with section name to make
    /// section nesting more readable.
    fn trace(&self, args: std::fmt::Arguments) {
        // Only evaluate and allocate strings if TRACE is actually enabled
        if tracing::enabled!(tracing::Level::TRACE) {
            let sec_depth = self.scope_stack.len();
            let sec_name = self
                .scope_stack
                .last()
                .map(|f| f.sec_name.as_str())
                .unwrap_or("");
            tracing::trace!("{}{}: {}", "    ".repeat(sec_depth), sec_name, args);
        }
    }

    fn iterate_wrs(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        current: &mut Location,
    ) -> bool {
        self.trace(format_args!("LayoutPhase::iterate_wrs: {}", current));

        let Some(xstr) = evaluate_string_expr(&self.parms, &irdb.parms, &ir.operands, diags) else {
            return false;
        };

        // Will panic if usize does not fit in u64
        let size = xstr.len() as u64;

        current.advance(size, &ir.src_loc, diags)
    }

    /// Advances the Location by the output size of an extension call.
    /// The ir is a `wr` with one operand: the extension identifier.
    fn iterate_ext(
        &mut self,
        ir: &IR,
        current: &mut Location,
        ext_registry: &ExtensionRegistry,
        diags: &mut Diags,
    ) -> bool {
        self.trace(format_args!("LayoutPhase::iterate_ext: {}", current));

        let ext_name = self.parms[ir.operands[0]].identifier_to_str();
        if let Some(entry) = ext_registry.get(ext_name) {
            let size = entry.cached_size as u64;
            return current.advance(size, &ir.src_loc, diags);
        }

        diags.err1(
            "ERR_168",
            &format!(
                "Failed to resolve extension '{}' size during layout.",
                ext_name
            ),
            ir.src_loc.clone(),
        );
        false
    }

    // Used for Wr8 though Wr64
    fn iterate_wrx(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        current: &mut Location,
    ) -> bool {
        assert!(ir.operands.len() < 3);
        let byte_size = get_wrx_byte_width(ir) as u64;

        self.trace(format_args!(
            "LayoutPhase::iterate_wrx-{}: {}",
            byte_size * 8,
            current
        ));

        let mut result = true;

        // determine the optional repeat count value
        let mut repeat_count = 1;
        if ir.operands.len() == 2 {
            // Yes, we have a repeat count
            // A repeat count of 0 is not an error.
            let op = &self.parms[ir.operands[1]];
            match op.data_type() {
                DataType::U64 => {
                    repeat_count = op.to_u64();
                }
                DataType::Integer | DataType::I64 => {
                    let temp = op.to_i64();
                    if temp < 0 {
                        let msg = format!(
                            "Repeat count cannot be negative, \
                                                but found '{}'",
                            temp
                        );
                        let src_loc = irdb.parms[ir.operands[1]].src_loc.clone();
                        diags.err1("ERR_154", &msg, src_loc);
                        result = false;
                        repeat_count = 0;
                    } else {
                        repeat_count = op.to_u64();
                    }
                }
                bad => {
                    let msg = format!("Repeat count cannot be type '{:?}'", bad);
                    let src_loc = irdb.parms[ir.operands[1]].src_loc.clone();
                    diags.err1("ERR_153", &msg, src_loc);
                    result = false;
                }
            }
        }

        // total size is the size of the wrx times the optional repeat count
        let Some(size) = byte_size.checked_mul(repeat_count) else {
            let src_loc = irdb.parms[if ir.operands.len() == 2 {
                ir.operands[1]
            } else {
                ir.operands[0]
            }]
            .src_loc
            .clone();
            diags.err1(
                "ERR_158",
                "Write repeat count causes size overflow",
                src_loc,
            );
            return false;
        };

        self.trace(format_args!(
            "LayoutPhase::iterate_wrx-{}: size is {}",
            byte_size * 8,
            size
        ));

        result &= current.advance(size, &ir.src_loc, diags);
        result
    }

    /// Used for wr file
    /// There is nothing really to iterate other than advancing
    /// the location counter by the size of the file.
    fn iterate_wrf(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        current: &mut Location,
    ) -> bool {
        self.trace(format_args!("LayoutPhase::iterate_wrf: {}", current));

        // The operand is a file path
        assert!(ir.operands.len() < 2);

        let path_opnd = &self.parms[ir.operands[0]];
        let file_path = path_opnd.to_str();

        // we already verified this is a legit file path,
        // so unwrap is ok.
        let file_info = irdb.files.get(file_path).unwrap();

        let byte_size = file_info.size;

        self.trace(format_args!(
            "LayoutPhase::iterate_wrf '{}' has size {}",
            file_path, byte_size
        ));

        current.advance(byte_size, &ir.src_loc, diags)
    }

    fn iterate_wrobj(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        current: &mut Location,
    ) -> bool {
        let obj_name = self.parms[ir.operands[0]].identifier_to_str().to_string();
        let infos = irdb.objsecs.get(&obj_name).unwrap();
        current.advance(packed_size(infos), &ir.src_loc, diags)
    }

    /// Compute the string representation of the expression.
    /// Returns the resulting string in xstr.
    /// If the diags noprint option is true, suppress printing.
    /// Returns None of failure
    fn do_u64_add(ir: &IR, in0: u64, in1: u64, out: &mut u64, diags: &mut Diags) -> bool {
        match in0.checked_add(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Add expression '{in0} + {in1}' will overflow type U64");
                diags.err1("ERR_125", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_i64_add(ir: &IR, in0: i64, in1: i64, out: &mut i64, diags: &mut Diags) -> bool {
        match in0.checked_add(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Add expression '{in0} + {in1}' will overflow type I64");
                diags.err1("ERR_145", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_u64_sub(ir: &IR, in0: u64, in1: u64, out: &mut u64, diags: &mut Diags) -> bool {
        match in0.checked_sub(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Subtract expression '{in0} - {in1}' will underflow type U64");
                diags.err1("ERR_128", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_i64_sub(ir: &IR, in0: i64, in1: i64, out: &mut i64, diags: &mut Diags) -> bool {
        match in0.checked_sub(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Subtract expression '{in0} - {in1}' will underflow type I64");
                diags.err1("ERR_147", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_u64_mul(ir: &IR, in0: u64, in1: u64, out: &mut u64, diags: &mut Diags) -> bool {
        match in0.checked_mul(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Multiply expression '{in0} * {in1}' will overflow type U64");
                diags.err1("ERR_130", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_i64_mul(ir: &IR, in0: i64, in1: i64, out: &mut i64, diags: &mut Diags) -> bool {
        match in0.checked_mul(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg =
                    format!("Multiply expression '{in0} * {in1}' will overflow data type I64");
                diags.err1("ERR_148", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_u64_div(ir: &IR, in0: u64, in1: u64, out: &mut u64, diags: &mut Diags) -> bool {
        match in0.checked_div(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Exception in divide expression '{in0} / {in1}'");
                diags.err1("ERR_131", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_u64_mod(ir: &IR, in0: u64, in1: u64, out: &mut u64, diags: &mut Diags) -> bool {
        match in0.checked_rem(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Exception in modulo expression '{in0} % {in1}'");
                diags.err1("ERR_150", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_i64_div(ir: &IR, in0: i64, in1: i64, out: &mut i64, diags: &mut Diags) -> bool {
        match in0.checked_div(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Exception in divide expression '{in0} / {in1}'");
                diags.err1("ERR_149", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_i64_mod(ir: &IR, in0: i64, in1: i64, out: &mut i64, diags: &mut Diags) -> bool {
        match in0.checked_rem(in1) {
            Some(v) => {
                *out = v;
                true
            }
            None if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            None => {
                let msg = format!("Exception in modulo expression '{in0} % {in1}'");
                diags.err1("ERR_152", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_u64_shl(ir: &IR, in0: u64, in1: u64, out: &mut u64, diags: &mut Diags) -> bool {
        match ir::check_shl_u64(in0, in1) {
            Ok(v) => {
                *out = v;
                true
            }
            Err(_) if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            Err(msg) => {
                diags.err1("ERR_133", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_i64_shl(ir: &IR, in0: i64, in1: i64, out: &mut i64, diags: &mut Diags) -> bool {
        match ir::check_shl_i64(in0, in1) {
            Ok(v) => {
                *out = v;
                true
            }
            Err(_) if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            Err(msg) => {
                diags.err1("ERR_151", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_u64_shr(ir: &IR, in0: u64, in1: u64, out: &mut u64, diags: &mut Diags) -> bool {
        match ir::check_shr_u64(in0, in1) {
            Ok(v) => {
                *out = v;
                true
            }
            Err(_) if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            Err(msg) => {
                diags.err1("ERR_134", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn do_i64_shr(ir: &IR, in0: i64, in1: i64, out: &mut i64, diags: &mut Diags) -> bool {
        match ir::check_shr_i64(in0, in1) {
            Ok(v) => {
                *out = v;
                true
            }
            Err(_) if diags.suppress_arith_err => {
                *out = 0;
                true
            }
            Err(msg) => {
                diags.err1("ERR_144", &msg, ir.src_loc.clone());
                false
            }
        }
    }

    fn iterate_type_conversion(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        operation: IRKind,
        current: &Location,
        diags: &mut Diags,
    ) -> bool {
        self.trace(format_args!(
            "LayoutPhase::iterate_type_conversion: {}",
            current
        ));
        // All operations here take one input and produce one output parameter
        let mut result = true;
        assert!(ir.operands.len() == 2);
        let in_parm_num0 = ir.operands[0];
        let out_parm_num = ir.operands[1];
        let in_parm0 = self.parms[in_parm_num0].clone();
        let out_parm = &mut self.parms[out_parm_num];
        match operation {
            IRKind::ToU64 => {
                let out = out_parm.to_u64_mut();
                match in_parm0.data_type() {
                    DataType::U64 => {
                        // Trivial Integer or U64 to U64
                        let in0 = in_parm0.to_u64();
                        *out = in0;
                    }
                    DataType::Integer | DataType::I64 => {
                        // I64 to U64
                        let in0 = in_parm0.to_i64();
                        *out = in0 as u64;
                    }
                    bad => {
                        let src_loc = irdb.parms[in_parm_num0].src_loc.clone();
                        let msg = format!("Can't convert from {bad:?} to U64");
                        diags.err1("ERR_141", &msg, src_loc);
                        result = false;
                    }
                }
            }
            IRKind::ToI64 => {
                let out = out_parm.to_i64_mut();
                match in_parm0.data_type() {
                    DataType::U64 => {
                        // U64 to I64
                        let in0 = in_parm0.to_u64();
                        *out = in0 as i64;
                    }
                    DataType::Integer | DataType::I64 => {
                        // Trivial Integer or I64 to I64
                        let in0 = in_parm0.to_i64();
                        *out = in0;
                    }
                    bad => {
                        let src_loc = irdb.parms[in_parm_num0].src_loc.clone();
                        let msg = format!("Can't convert from {bad:?} to I64");
                        diags.err1("ERR_136", &msg, src_loc);
                        result = false;
                    }
                }
            }

            bad => {
                panic!("Called iterate_type_conversion with bad IRKind operation {bad:?}");
            }
        }
        result
    }

    fn iterate_arithmetic(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        operation: IRKind,
        current: &Location,
        diags: &mut Diags,
    ) -> bool {
        self.trace(format_args!("LayoutPhase::iterate_arithmetic: {}", current));
        // All operations here take two inputs and produces one output parameter
        assert!(ir.operands.len() == 3);

        // Borrow the parameters from the main array
        let lhs_num = ir.operands[0];
        let rhs_num = ir.operands[1];
        let out_num = ir.operands[2];
        let lhs = &self.parms[lhs_num];
        let rhs = &self.parms[rhs_num];

        let lhs_dt = lhs.data_type();
        let rhs_dt = rhs.data_type();

        if lhs_dt != rhs_dt {
            let mut dt_ok = false;
            // Right and left side data types are not equal.
            // Determine if we can proceed.
            if rhs_dt == DataType::Integer {
                if [DataType::I64, DataType::U64, DataType::Integer].contains(&lhs_dt) {
                    dt_ok = true; // Integers work with s/u types
                }
            } else if lhs_dt == DataType::Integer
                && [DataType::I64, DataType::U64].contains(&rhs_dt)
            {
                dt_ok = true; // Integers work with s/u types
            }

            if !dt_ok {
                let loc0 = irdb.parms[lhs_num].src_loc.clone();
                let loc1 = irdb.parms[rhs_num].src_loc.clone();
                let msg = format!(
                    "Input operand types do not match.  Left is '{:?}', right is '{:?}'",
                    lhs_dt, rhs_dt
                );
                diags.err2("ERR_137", &msg, loc0, loc1);
                return false;
            }
        }

        let mut result = true;
        // output of compare is u64 regardless of inputs
        // check both parms since one might be an ambiguous integer
        // If either side is unsigned, the whole thing is unsigned
        if (lhs_dt == DataType::U64) || (rhs_dt == DataType::U64) {
            let in0 = lhs.to_u64();
            let in1 = rhs.to_u64();
            let out_parm = &mut self.parms[out_num];
            let out = out_parm.to_u64_mut();

            match operation {
                IRKind::DoubleEq => *out = (in0 == in1) as u64,
                IRKind::NEq => *out = (in0 != in1) as u64,
                IRKind::GEq => *out = (in0 >= in1) as u64,
                IRKind::LEq => *out = (in0 <= in1) as u64,
                IRKind::Gt => *out = (in0 > in1) as u64,
                IRKind::Lt => *out = (in0 < in1) as u64,
                IRKind::BitAnd => *out = in0 & in1,
                IRKind::LogicalAnd => *out = ((in0 != 0) && (in1 != 0)) as u64,
                IRKind::BitOr => *out = in0 | in1,
                IRKind::LogicalOr => *out = ((in0 != 0) || (in1 != 0)) as u64,
                IRKind::Add => {
                    result &= LayoutPhase::do_u64_add(ir, in0, in1, out, diags);
                }
                IRKind::Subtract => {
                    result &= LayoutPhase::do_u64_sub(ir, in0, in1, out, diags);
                }
                IRKind::Multiply => {
                    result &= LayoutPhase::do_u64_mul(ir, in0, in1, out, diags);
                }
                IRKind::Divide => {
                    result &= LayoutPhase::do_u64_div(ir, in0, in1, out, diags);
                }
                IRKind::Modulo => {
                    result &= LayoutPhase::do_u64_mod(ir, in0, in1, out, diags);
                }
                IRKind::LeftShift => {
                    result &= LayoutPhase::do_u64_shl(ir, in0, in1, out, diags);
                }
                IRKind::RightShift => {
                    result &= LayoutPhase::do_u64_shr(ir, in0, in1, out, diags);
                }
                bad => panic!("Forgot to handle u64 {:?}", bad),
            };
        } else if (lhs_dt == DataType::I64)
            || (rhs_dt == DataType::I64)
            || ((lhs_dt == DataType::Integer) && (rhs_dt == DataType::Integer))
        {
            // If either side is signed, treat the whole expression as signed
            // If both sides are ambiguous integers then treat the whole expression as signed
            let in0 = lhs.to_i64();
            let in1 = rhs.to_i64();
            let out_parm = &mut self.parms[out_num];

            match operation {
                // output of compare is u64 regardless of inputs
                IRKind::LogicalAnd => {
                    let out = out_parm.to_u64_mut();
                    *out = ((in0 != 0) && (in1 != 0)) as u64
                }
                IRKind::LogicalOr => {
                    let out = out_parm.to_u64_mut();
                    *out = ((in0 != 0) || (in1 != 0)) as u64
                }
                IRKind::LEq => {
                    let out = out_parm.to_u64_mut();
                    *out = (in0 <= in1) as u64
                }
                IRKind::GEq => {
                    let out = out_parm.to_u64_mut();
                    *out = (in0 >= in1) as u64
                }
                IRKind::Gt => {
                    let out = out_parm.to_u64_mut();
                    *out = (in0 > in1) as u64
                }
                IRKind::Lt => {
                    let out = out_parm.to_u64_mut();
                    *out = (in0 < in1) as u64
                }
                IRKind::NEq => {
                    let out = out_parm.to_u64_mut();
                    *out = (in0 != in1) as u64
                }
                IRKind::DoubleEq => {
                    let out = out_parm.to_u64_mut();
                    *out = (in0 == in1) as u64
                }

                IRKind::BitOr => {
                    let out = out_parm.to_i64_mut();
                    *out = in0 | in1
                }
                IRKind::BitAnd => {
                    let out = out_parm.to_i64_mut();
                    *out = in0 & in1
                }
                IRKind::Add => {
                    let out = out_parm.to_i64_mut();
                    result &= LayoutPhase::do_i64_add(ir, in0, in1, out, diags);
                }
                IRKind::Subtract => {
                    let out = out_parm.to_i64_mut();
                    result &= LayoutPhase::do_i64_sub(ir, in0, in1, out, diags);
                }
                IRKind::Multiply => {
                    let out = out_parm.to_i64_mut();
                    result &= LayoutPhase::do_i64_mul(ir, in0, in1, out, diags);
                }
                IRKind::Divide => {
                    let out = out_parm.to_i64_mut();
                    result &= LayoutPhase::do_i64_div(ir, in0, in1, out, diags);
                }
                IRKind::Modulo => {
                    let out = out_parm.to_i64_mut();
                    result &= LayoutPhase::do_i64_mod(ir, in0, in1, out, diags);
                }
                IRKind::LeftShift => {
                    let out = out_parm.to_i64_mut();
                    result &= LayoutPhase::do_i64_shl(ir, in0, in1, out, diags);
                }
                IRKind::RightShift => {
                    let out = out_parm.to_i64_mut();
                    result &= LayoutPhase::do_i64_shr(ir, in0, in1, out, diags);
                }

                bad => panic!("Forgot to handle i64 {:?}", bad),
            }
        } else {
            let loc0 = irdb.parms[lhs_num].src_loc.clone();
            let loc1 = irdb.parms[rhs_num].src_loc.clone();
            // check above ensures the types are the same, whatever they are
            let msg = format!(
                "Unexpected input operand types '{:?}'  Expected I64 or U64.",
                lhs_dt
            );
            diags.err2("ERR_143", &msg, loc0, loc1);
            return false;
        }
        result
    }

    fn iterate_sizeof(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        current: &Location,
    ) -> bool {
        self.trace(format_args!("LayoutPhase::iterate_sizeof: {}", current));

        // sizeof takes one input and produces one output
        // we've already discarded surrounding () on the operand
        assert!(ir.operands.len() == 2);
        let in_parm_num0 = ir.operands[0]; // identifier
        let out_parm_num = ir.operands[1];

        let in_parm = &self.parms[in_parm_num0];

        if in_parm.data_type() == DataType::DeferredRef {
            let sec_name = in_parm.identifier_to_str().to_string();

            // Section path: derive size from layout-phase ir_locs.
            if let Some(ir_rng) = irdb.sized_locs.get(&sec_name) {
                assert!(ir_rng.start <= ir_rng.end);
                let start_loc = &self.ir_locs[ir_rng.start];
                let end_loc = &self.ir_locs[ir_rng.end];

                if start_loc.file_offset > end_loc.file_offset {
                    // On the first iteration a section may not yet have a valid
                    // end location (e.g. sizeof() of self).  Return 0 so the
                    // loop converges on the next pass.
                    self.trace(format_args!(
                        "Section {}: Starting file_pos {} > ending file_pos {}",
                        sec_name, start_loc.file_offset, end_loc.file_offset
                    ));
                    *self.parms[out_parm_num].to_u64_mut() = 0;
                } else {
                    let sz: u64 = end_loc.file_offset - start_loc.file_offset;
                    self.trace(format_args!("Sizeof {} is currently {}", sec_name, sz));
                    *self.parms[out_parm_num].to_u64_mut() = sz;
                }
                return true;
            }

            // Obj path: size is the packed total of all matched sections.
            if let Some(infos) = irdb.objsecs.get(&sec_name) {
                *self.parms[out_parm_num].to_u64_mut() = packed_size(infos);
                return true;
            }

            // Region path: size is const-evaluated and stable across iterations.
            if let Some(binding) = irdb.region_props.get(&sec_name) {
                *self.parms[out_parm_num].to_u64_mut() = binding.size;
            } else {
                unreachable!(
                    "sizeof() argument '{}' is not a section used in output or \
                     a declared obj or region.  layoutdb::verify_operand_refs should have \
                     caught this.",
                    sec_name
                );
            }
            return true;
        }

        diags.err1(
            "ERR_169",
            "sizeof() only accepts section or region names.",
            ir.src_loc.clone(),
        );
        false
    }

    fn iterate_sizeof_ext(
        &mut self,
        ir: &IR,
        diags: &mut Diags,
        ext_registry: &ExtensionRegistry,
    ) -> bool {
        // SizeofExt has 2 operands: [0] = extension name (Identifier), [1] = output U64
        assert!(ir.operands.len() == 2);
        let out_parm_num = ir.operands[1];
        let name = self.parms[ir.operands[0]].identifier_to_str().to_string();
        if let Some(entry) = ext_registry.get(&name) {
            *self.parms[out_parm_num].to_u64_mut() = entry.cached_size as u64;
            true
        } else {
            diags.err1(
                "ERR_170",
                &format!("Unknown extension '{}' in sizeof().", name),
                ir.src_loc.clone(),
            );
            false
        }
    }

    fn iterate_obj_align(&mut self, ir: &IR, irdb: &IRDb) -> bool {
        assert!(ir.operands.len() == 2);
        let obj_name = self.parms[ir.operands[0]].identifier_to_str().to_string();
        let info = &irdb.objsecs.get(&obj_name).unwrap()[0];
        *self.parms[ir.operands[1]].to_u64_mut() = info.align;
        true
    }

    fn iterate_obj_vma(&mut self, ir: &IR, irdb: &IRDb) -> bool {
        assert!(ir.operands.len() == 2);
        let obj_name = self.parms[ir.operands[0]].identifier_to_str().to_string();
        let info = &irdb.objsecs.get(&obj_name).unwrap()[0];
        *self.parms[ir.operands[1]].to_u64_mut() = info.vma;
        true
    }

    fn iterate_obj_lma(&mut self, ir: &IR, irdb: &IRDb) -> bool {
        assert!(ir.operands.len() == 2);
        let obj_name = self.parms[ir.operands[0]].identifier_to_str().to_string();
        let info = &irdb.objsecs.get(&obj_name).unwrap()[0];
        *self.parms[ir.operands[1]].to_u64_mut() = info.lma;
        true
    }

    fn iterate_output_size(&mut self, ir: &IR, irdb: &IRDb, diags: &mut Diags) -> bool {
        // __OUTPUT_SIZE has no input operands, only a single output operand.
        assert!(ir.operands.len() == 1);
        let out_parm_num = ir.operands[0];
        let sec_name = &irdb.output_sec_str;

        let Some(ir_rng) = irdb.sized_locs.get(sec_name) else {
            let msg = format!("__OUTPUT_SIZE: output section '{}' not found.", sec_name);
            diags.err1("ERR_174", &msg, ir.src_loc.clone());
            return false;
        };
        assert!(ir_rng.start <= ir_rng.end);
        let start_loc = &self.ir_locs[ir_rng.start];
        let end_loc = &self.ir_locs[ir_rng.end];

        if start_loc.file_offset > end_loc.file_offset {
            *self.parms[out_parm_num].to_u64_mut() = 0;
        } else {
            *self.parms[out_parm_num].to_u64_mut() = end_loc.file_offset - start_loc.file_offset;
        }
        true
    }

    fn iterate_output_addr(&mut self, ir: &IR, irdb: &IRDb, diags: &mut Diags) -> bool {
        // __OUTPUT_ADDR has no input operands, only a single output operand.
        assert!(ir.operands.len() == 1);
        let out_parm_num = ir.operands[0];
        let sec_name = &irdb.output_sec_str;

        let Some(ir_num) = irdb.addressed_locs.get(sec_name) else {
            let msg = format!(
                "__OUTPUT_ADDR: output section '{}' not reachable.",
                sec_name
            );
            diags.err1("ERR_175", &msg, ir.src_loc.clone());
            return false;
        };
        let start_loc = &self.ir_locs[*ir_num];

        let Some(val) = start_loc
            .addr
            .addr_base
            .checked_add(start_loc.addr.addr_offset)
        else {
            diags.err1(
                "ERR_176",
                "__OUTPUT_ADDR: address overflow.",
                ir.src_loc.clone(),
            );
            return false;
        };
        *self.parms[out_parm_num].to_u64_mut() = val;
        true
    }

    fn iterate_builtin_version_string(&mut self, ir: &IR) -> bool {
        assert!(ir.operands.len() == 1);
        self.parms[ir.operands[0]] =
            ParameterValue::QuotedString(ConstBuiltins::get().firmion_version_string.to_string());
        true
    }

    fn iterate_builtin_version_major(&mut self, ir: &IR) -> bool {
        assert!(ir.operands.len() == 1);
        *self.parms[ir.operands[0]].to_u64_mut() = ConstBuiltins::get().firmion_version_major;
        true
    }

    fn iterate_builtin_version_minor(&mut self, ir: &IR) -> bool {
        assert!(ir.operands.len() == 1);
        *self.parms[ir.operands[0]].to_u64_mut() = ConstBuiltins::get().firmion_version_minor;
        true
    }

    fn iterate_builtin_version_patch(&mut self, ir: &IR) -> bool {
        assert!(ir.operands.len() == 1);
        *self.parms[ir.operands[0]].to_u64_mut() = ConstBuiltins::get().firmion_version_patch;
        true
    }

    /// Compute the transient current address.  This case is called when
    /// addr/addr_offset/sec_offset is called without an identifier.
    fn iterate_current_address(&mut self, ir: &IR, diags: &mut Diags, current: &Location) -> bool {
        self.trace(format_args!(
            "LayoutPhase::iterate_current_address: {}",
            current
        ));
        assert!(ir.operands.len() == 1);
        let out_parm_num = ir.operands[0];
        let out_parm = &mut self.parms[out_parm_num];
        let out = out_parm.to_u64_mut();

        match ir.kind {
            IRKind::Addr => {
                let Some(val) = current.addr.addr_base.checked_add(current.addr.addr_offset) else {
                    diags.err1(
                        "ERR_161",
                        "Absolute address (abs_base + off) overflow",
                        ir.src_loc.clone(),
                    );
                    return false;
                };
                *out = val;
            }
            IRKind::AddrOffset => {
                *out = current.addr.addr_offset;
            }
            IRKind::SecOffset => {
                *out = current.addr.sec_offset;
            }
            IRKind::FileOffset => {
                *out = current.file_offset;
            }
            bad => {
                panic!("Called iterate_current_address with bogus IR {:?}", bad);
            }
        }

        true
    }

    /// Compute the required number of bytes to align the current absolute location.
    /// We don't actually align anything yet, since that happens in a subsequent
    /// wr8 instruction.
    fn iterate_align(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        current: &Location,
    ) -> bool {
        self.trace(format_args!("LayoutPhase::iterate_align: {}", current));

        let num_operands = ir.operands.len();

        // The first parameter is the align amount
        // The optional second parameter is the pad value.  We don't care about
        // the pad value anymore, since that parameter pertains only to the wr8.
        // The final parameter is the result operand for the number
        // of bytes required to align.
        assert!(num_operands == 2 || num_operands == 3);
        let out_parm_num = if num_operands == 2 {
            ir.operands[1]
        } else {
            ir.operands[2]
        };

        let align_parm_num = ir.operands[0];
        let align_val = self.parms[align_parm_num].to_u64();

        if align_val == 0 {
            // Align 0 causes division by zero at checked_rem.
            // During convergence the value may be transiently zero (e.g. sizeof
            // not yet settled); suppress and emit 0 padding bytes so the loop
            // can converge, then the strict final pass will catch genuine zeros.
            if diags.suppress_arith_err {
                *self.parms[out_parm_num].to_u64_mut() = 0;
                return true;
            }
            let src_loc = irdb.parms[align_parm_num].src_loc.clone();
            diags.err1("ERR_160", "Alignment amount cannot be zero", src_loc);
            return false;
        }

        let out_parm = &mut self.parms[out_parm_num];
        let out = out_parm.to_u64_mut();

        let Some(abs_val) = current.addr.addr_base.checked_add(current.addr.addr_offset) else {
            diags.err1(
                "ERR_164",
                "Absolute address (abs_base + off) overflow",
                ir.src_loc.clone(),
            );
            return false;
        };

        let remainder = abs_val.checked_rem(align_val).unwrap();

        *out = if remainder == 0 {
            0 // we're already aligned, no pad bytes needed
        } else {
            align_val - remainder
        };

        debug!("LayoutPhase::iterate_align: alignment amount is {}", *out);
        true
    }

    /// Compute the required number of bytes to pad the current section to the specified size.
    /// We don't actually pad anything yet, since that happens in a subsequent
    /// wr8 instruction.
    /// This function covers pad_sec_offset and pad_addr_offset.
    fn iterate_set(
        &mut self,
        ir: &IR,
        _irdb: &IRDb,
        diags: &mut Diags,
        current: &Location,
    ) -> bool {
        self.trace(format_args!(
            "LayoutPhase::iterate_set: {:?}: {}",
            ir.kind, current
        ));

        let num_operands = ir.operands.len();

        // The first parameter is the pad amount
        // The optional second parameter is the pad value.  We don't care about
        // the pad value anymore, since that parameter pertains only to the wr8.
        // The final parameter is the result operand for the number
        // of bytes required to pad.
        assert!(num_operands == 2 || num_operands == 3);
        let out_parm_num = if num_operands == 2 {
            ir.operands[1]
        } else {
            ir.operands[2]
        };

        let set_parm_num = ir.operands[0];
        let set_val = self.parms[set_parm_num].to_u64();

        let out_parm = &mut self.parms[out_parm_num];
        let out = out_parm.to_u64_mut();

        let loc = match ir.kind {
            IRKind::SetAddrOffset => current.addr.addr_offset,
            IRKind::SetSecOffset => current.addr.sec_offset,
            IRKind::SetFileOffset => current.file_offset,
            bad => panic!("called iterate_set for IR {:?}", bad),
        };

        // The current location can never move backwards.
        // During convergence the target value may be transiently too small (e.g.
        // sizeof not yet settled); suppress and emit 0 padding bytes so the loop
        // can converge, then the strict final pass will catch genuine reversals.
        if set_val < loc {
            if diags.suppress_arith_err {
                *out = 0;
                return true;
            }
            let msg = format!(
                "Set statement moves location counter backwards from {} to {}.",
                loc, set_val
            );
            diags.err1("ERR_146", &msg, ir.src_loc.clone());
            return false;
        }

        *out = set_val - loc;

        debug!(
            "LayoutPhase::iterate_set: {:?} set amount is {}",
            ir.kind, *out
        );
        true
    }

    /// Handle `set_addr(X)`: pure cursor rebase.
    /// Sets abs_base = X and resets off = 0.  No bytes are emitted.
    /// Backward rebase is valid (firmware load-address use case).
    fn iterate_set_addr(
        &mut self,
        ir: &IR,
        _irdb: &IRDb,
        lid: usize,
        diags: &mut Diags,
        current: &mut Location,
    ) -> bool {
        let set_parm_num = ir.operands[0];
        let set_val = self.parms[set_parm_num].to_u64();

        self.trace(format_args!(
            "LayoutPhase::iterate_set_addr: abs_base {} -> {}, addr_offset is reset to 0",
            current.addr.addr_base, set_val
        ));

        let num_operands = ir.operands.len();
        assert!(num_operands == 2 || num_operands == 3);
        let out_parm_num = if num_operands == 2 {
            ir.operands[1]
        } else {
            ir.operands[2]
        };

        // Check that the target address is within the effective region constraint,
        // which is the intersection of all ancestor and direct region bindings.
        // Skip during convergence: set_val may be transiently wrong (e.g. when
        // sizeof is not yet settled), and the strict final pass will catch genuine
        // out-of-bounds addresses.
        if !diags.suppress_arith_err
            && let Some(frame) = self.scope_stack.last()
            && let Some(effective) = frame.effective_region.as_ref()
            && !effective.contains_addr(set_val)
        {
            if self.warned_lids.insert((lid, "ERR_185")) {
                let b = &effective.effective_region;
                let msg = format!(
                    "set_addr target {:#X} is outside region '{}' bounds [{:#X}, {:#X}).",
                    set_val,
                    b.name,
                    b.addr,
                    b.addr + b.size
                );
                diags.err2("ERR_185", &msg, ir.src_loc.clone(), b.src_loc.clone());
            }
            return false;
        }

        // Record that set_addr was called mid-section if sec_offset is non-zero.
        // This arms the warning for any subsequent pad_sec_offset in this scope.
        if current.addr.sec_offset != 0
            && let Some(frame) = self.scope_stack.last_mut()
        {
            frame.set_addr_seen = true;
        }

        current.addr.addr_base = set_val;
        current.addr.addr_offset = 0;

        // No bytes to write; tell the consumer wr8 to emit 0 bytes.
        *self.parms[out_parm_num].to_u64_mut() = 0;

        true
    }

    /// Compute the transient address of the identifier.  This case is called when
    /// addr/addr_offset/sec_offset is called with an identifier.
    fn iterate_identifier_address(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        current: &Location,
    ) -> bool {
        self.trace(format_args!(
            "LayoutPhase::iterate_identifier_address: {}",
            current
        ));

        // addr/addr_offset/sec_offset take one optional input and produce one output.
        // We've already discarded surrounding () on the operand.
        assert!(ir.operands.len() == 2);
        let in_parm_num0 = ir.operands[0]; // identifier
        let out_parm_num = ir.operands[1];

        let name = self.parms[in_parm_num0].identifier_to_str().to_string();

        // Section / label path: look up the ir_locs entry for layout-time values.
        if let Some(ir_num) = irdb.addressed_locs.get(&name) {
            let start_loc = &self.ir_locs[*ir_num];
            match ir.kind {
                IRKind::Addr => {
                    let Some(val) = start_loc
                        .addr
                        .addr_base
                        .checked_add(start_loc.addr.addr_offset)
                    else {
                        diags.err1(
                            "ERR_166",
                            "Absolute address (abs_base + off) overflow for identifier",
                            ir.src_loc.clone(),
                        );
                        return false;
                    };
                    *self.parms[out_parm_num].to_u64_mut() = val;
                }
                IRKind::AddrOffset => {
                    *self.parms[out_parm_num].to_u64_mut() = start_loc.addr.addr_offset;
                }
                IRKind::SecOffset => {
                    *self.parms[out_parm_num].to_u64_mut() = start_loc.addr.sec_offset;
                }
                IRKind::FileOffset => {
                    *self.parms[out_parm_num].to_u64_mut() = start_loc.file_offset;
                }
                bad => {
                    panic!("Called iterate_identifier_address with bogus IR {:?}", bad);
                }
            }
            return true;
        }

        // Region path: addr and size are const-evaluated, not layout-dependent.
        // Only addr(REGION) is valid; the offset variants have no meaning for a
        // static region declaration.
        if let Some(binding) = irdb.region_props.get(&name) {
            match ir.kind {
                IRKind::Addr => {
                    *self.parms[out_parm_num].to_u64_mut() = binding.addr;
                    return true;
                }
                IRKind::AddrOffset | IRKind::SecOffset | IRKind::FileOffset => {
                    let kind_str = match ir.kind {
                        IRKind::AddrOffset => "addr_offset",
                        IRKind::SecOffset => "sec_offset",
                        IRKind::FileOffset => "file_offset",
                        _ => unreachable!(),
                    };
                    let msg = format!(
                        "{}({}) is not valid for region '{}'; use addr({}) instead.",
                        kind_str, name, name, name
                    );
                    diags.err1("ERR_189", &msg, ir.src_loc.clone());
                    return false;
                }
                bad => {
                    panic!("Called iterate_identifier_address with bogus IR {:?}", bad);
                }
            }
        }

        let msg = format!("Section, label, or region '{}' not found in output.", name);
        diags.err1("ERR_135", &msg, ir.src_loc.clone());
        false
    }

    fn iterate_address(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        diags: &mut Diags,
        current: &Location,
    ) -> bool {
        self.trace(format_args!("LayoutPhase::iterate_address: {}", current));

        // addr/addr_offset/sec_offset take one optional input and produce one output.
        // We've already discarded surrounding () on the operand.
        let num_operands = ir.operands.len();

        match num_operands {
            1 => self.iterate_current_address(ir, diags, current),
            2 => self.iterate_identifier_address(ir, irdb, diags, current),
            bad => panic!("Wrong number of IR operands = {}!", bad),
        }
    }

    /// On section entry, look up the pre-computed effective region from RegionDb,
    /// push the scope frame, and anchor the address base for region-bound sections.
    fn iterate_section_start(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        region_db: &RegionDb,
        current: &mut Location,
    ) {
        let sec_name = irdb.get_opnd_as_identifier(ir, 0);

        // Region intersections are pre-computed and stable -- no recalculation.
        let effective_region = region_db.effective_regions.get(sec_name).cloned();

        self.scope_stack.push(ScopeFrame {
            parent_state: current.addr.clone(),
            sec_name: sec_name.to_string(),
            set_addr_seen: false,
            effective_region,
        });
        self.trace(format_args!(
            "LayoutPhase::iterate_section_start: section \"{}\", {}",
            sec_name, current
        ));
        current.addr.sec_offset = 0;

        // Anchor to the direct region's addr.
        // Inner sections without a direct binding start wherever the cursor
        // is inside the parent section.
        if let Some(d) = irdb.region_for_section(sec_name) {
            current.addr.addr_base = d.addr;
            current.addr.addr_offset = 0;
        }
    }

    /// On section exit, restore parent location state and advance the parent's
    /// offsets by the child's byte count.
    fn iterate_section_end(
        &mut self,
        ir: &IR,
        irdb: &IRDb,
        _diags: &mut Diags,
        current: &mut Location,
    ) -> bool {
        let child_size = current.addr.sec_offset;
        let frame = self.scope_stack.pop().unwrap();
        self.trace(format_args!(
            "LayoutPhase::iterate_section_end: '{}', child_size {}, {}",
            irdb.get_opnd_as_identifier(ir, 0),
            child_size,
            current
        ));
        // No change to file offset at the end of a section, but we must restore
        // the parent's address state, then advance the parent's offsets by the
        // child's section offset.
        current.addr = frame.parent_state;
        current.addr.advance(child_size);

        true
    }

    pub fn build(
        irdb: &IRDb,
        region_db: &RegionDb,
        ext_registry: &ExtensionRegistry,
        diags: &mut Diags,
    ) -> anyhow::Result<(LocationDb, ParmValDb)> {
        // The first iterate loop may access any IR location, so initialize all
        // ir_locs locations to zero.
        let ir_locs = vec![
            Location {
                file_offset: 0,
                addr: AddressState {
                    addr_offset: 0,
                    sec_offset: 0,
                    addr_base: 0,
                }
            };
            irdb.ir_vec.len()
        ];

        let mut layout_phase = LayoutPhase {
            parms: Vec::new(),
            ir_locs,
            scope_stack: Vec::new(),
            warned_lids: HashSet::new(),
        };
        layout_phase.trace(format_args!("LayoutPhase::new"));

        // Initialize parameters from the IR operands.
        layout_phase.parms.reserve(irdb.parms.len());
        for opnd in &irdb.parms {
            layout_phase.parms.push(opnd.clone_val());
        }

        // This step iterates until the address locations stabilize.  At that
        // point, we know the final layout of the output file.
        let result = layout_phase.iterate(irdb, region_db, ext_registry, diags);
        if !result {
            anyhow::bail!("LayoutPhase construction failed.");
        }

        if !layout_phase.validate_section_regions(irdb, region_db, diags) {
            anyhow::bail!("LayoutPhase construction failed.");
        }

        // Now that locations are known, we build the location database.
        layout_phase.trace(format_args!("LayoutPhase::new: EXIT"));
        Ok((
            LocationDb {
                ir_locs: layout_phase.ir_locs,
            },
            ParmValDb::new(layout_phase.parms),
        ))
    }

    /// After iterate converges, verify each region-bound section fits within
    /// its effective region.
    fn validate_section_regions(
        &self,
        irdb: &IRDb,
        region_db: &RegionDb,
        diags: &mut Diags,
    ) -> bool {
        let mut result = true;
        for sec_name in irdb.section_region_names.keys() {
            let Some(ir_rng) = irdb.sized_locs.get(sec_name) else {
                continue;
            };
            let start_loc = &self.ir_locs[ir_rng.start];
            let end_loc = &self.ir_locs[ir_rng.end];
            let sec_size = end_loc.file_offset.saturating_sub(start_loc.file_offset);

            let Some(eff) = region_db.effective_regions.get(sec_name.as_str()) else {
                continue;
            };

            if !eff.check_section_fits(
                sec_name,
                sec_size,
                irdb.ir_vec[ir_rng.start].src_loc.clone(),
                diags,
            ) {
                result = false;
            }
        }
        result
    }

    pub fn dump_locations(&self) {
        for (idx, loc) in self.ir_locs.iter().enumerate() {
            debug!("{}: {:?}", idx, loc);
        }
    }

    /// Executes one full pass over `irdb.ir_vec`, updating `self.ir_locs` and
    /// `self.parms` in place.  Returns `false` if any instruction fails.
    fn run_one_pass(
        &mut self,
        irdb: &IRDb,
        region_db: &RegionDb,
        ext_registry: &ExtensionRegistry,
        diags: &mut Diags,
    ) -> bool {
        let mut result = true;
        let mut current = Location {
            file_offset: 0,
            addr: AddressState {
                addr_offset: 0,
                sec_offset: 0,
                addr_base: 0,
            },
        };

        assert!(self.scope_stack.is_empty());

        for (lid, ir) in irdb.ir_vec.iter().enumerate() {
            debug!(
                "LayoutPhase::iterate on lid {} at file_pos {}",
                lid, current.file_offset
            );
            // record our location after each IR
            self.ir_locs[lid] = current.clone();
            let operation = ir.kind;
            result &= match operation {
                // Arithmetic with two operands in, one out
                IRKind::Add
                | IRKind::Subtract
                | IRKind::RightShift
                | IRKind::LeftShift
                | IRKind::BitAnd
                | IRKind::LogicalAnd
                | IRKind::BitOr
                | IRKind::LogicalOr
                | IRKind::Multiply
                | IRKind::Divide
                | IRKind::Modulo
                | IRKind::DoubleEq
                | IRKind::GEq
                | IRKind::LEq
                | IRKind::Gt
                | IRKind::Lt
                | IRKind::NEq => self.iterate_arithmetic(ir, irdb, operation, &current, diags),
                IRKind::ToI64 | IRKind::ToU64 => {
                    self.iterate_type_conversion(ir, irdb, operation, &current, diags)
                }
                IRKind::Sizeof => self.iterate_sizeof(ir, irdb, diags, &current),
                IRKind::SizeofExt => self.iterate_sizeof_ext(ir, diags, ext_registry),
                IRKind::ObjAlign => self.iterate_obj_align(ir, irdb),
                IRKind::ObjLma   => self.iterate_obj_lma(ir, irdb),
                IRKind::ObjVma   => self.iterate_obj_vma(ir, irdb),
                IRKind::BuiltinOutputSize => self.iterate_output_size(ir, irdb, diags),
                IRKind::BuiltinOutputAddr => self.iterate_output_addr(ir, irdb, diags),
                IRKind::BuiltinVersionString => self.iterate_builtin_version_string(ir),
                IRKind::BuiltinVersionMajor => self.iterate_builtin_version_major(ir),
                IRKind::BuiltinVersionMinor => self.iterate_builtin_version_minor(ir),
                IRKind::BuiltinVersionPatch => self.iterate_builtin_version_patch(ir),

                // Unlike print, we have to iterate on the string write operation since
                // the size of the string affects the size of the output image.
                IRKind::Addr | IRKind::AddrOffset | IRKind::SecOffset | IRKind::FileOffset => {
                    self.iterate_address(ir, irdb, diags, &current)
                }
                IRKind::Wrs => self.iterate_wrs(ir, irdb, diags, &mut current),
                IRKind::SectionStart => {
                    self.iterate_section_start(ir, irdb, region_db, &mut current);
                    // Re-record after iterate_section_start so that addr(section_name)
                    // reflects the anchored address, not the pre-entry address.
                    self.ir_locs[lid] = current.clone();
                    true
                }
                IRKind::SectionEnd => self.iterate_section_end(ir, irdb, diags, &mut current),

                IRKind::Wr(_, _) => self.iterate_wrx(ir, irdb, diags, &mut current),
                IRKind::ExtensionCall => {
                    self.iterate_ext(ir, &mut current, ext_registry, diags)
                }
                IRKind::Align => self.iterate_align(ir, irdb, diags, &current),
                IRKind::SetSecOffset | IRKind::SetAddrOffset | IRKind::SetFileOffset => {
                    if ir.kind != IRKind::SetFileOffset
                        && let Some(true) = self.scope_stack.last().map(|f| &f.set_addr_seen)
                            && self.warned_lids.insert((lid, "ERR_171")) {
                                let cmd = if ir.kind == IRKind::SetSecOffset {
                                    "pad_sec_offset"
                                } else {
                                    "pad_addr_offset"
                                };
                                let msg = format!(
                                    "[ERR_171] Warning: '{}' follows 'set_addr' in the same \
                                     section scope.  '{}' pads to a sec/addr_offset value, \
                                     not to an address.  Consider 'pad_addr_offset' after \
                                     'set_addr' to pad relative to the address anchor.",
                                    cmd, cmd
                                );
                                diags.warn1("ERR_171", &msg, ir.src_loc.clone());
                            }
                    self.iterate_set(ir, irdb, diags, &current)
                }
                IRKind::SetAddr => self.iterate_set_addr(ir, irdb, lid, diags, &mut current),

                IRKind::Wrf => self.iterate_wrf(ir, irdb, diags, &mut current),
                IRKind::Wrobj => self.iterate_wrobj(ir, irdb, diags, &mut current),

                IRKind::Trace if diags.trace_enabled() => {
                    if let Some(mut s) = evaluate_string_expr(
                        &self.parms,
                        &irdb.parms,
                        &ir.operands,
                        diags,
                    ) {
                        let prefix = format!("[Trace-{}] ", diags.trace_iteration);
                        s.insert_str(0, &prefix);
                        print!("{}", s);
                    }
                    true
                }

                // The following IR types are evaluated only at execute time.
                // Nothing to do during iteration.
                IRKind::Const
                | IRKind::Eq
                | IRKind::Label
                | IRKind::Assert
                | IRKind::Print
                | IRKind::Trace
                | IRKind::I64
                | IRKind::U64
                // if/else IR only lives in const_ir_vec; never reaches the layout_phase.
                | IRKind::ConstDeclare
                | IRKind::IfBegin
                | IRKind::ElseBegin
                | IRKind::IfEnd
                | IRKind::BareAssign
                | IRKind::Output => true,
            }
        }

        result
    }

    /// Repeatedly executes the IR until all location-dependent values
    /// (addresses, alignments, section sizes) stabilize.  Each pass walks the
    /// full `irdb.ir_vec` in order, updating `self.ir_locs` with the image and
    /// section offset recorded after each instruction.  Because an alignment or
    /// `sizeof` expression may change the size of an earlier section on a later
    /// pass, iteration continues until two consecutive passes produce identical
    /// location vectors.
    ///
    /// Convergence passes run with `suppress_arith_err = true` so that transient
    /// underflow/overflow from not-yet-known layout values (e.g. sizeof returning 0
    /// in iteration 1) does not abort the loop.  After the fixed point is reached,
    /// one final strict pass with `suppress_arith_err = false` confirms that the
    /// converged values produce no arithmetic errors.
    pub fn iterate(
        &mut self,
        irdb: &IRDb,
        region_db: &RegionDb,
        ext_registry: &ExtensionRegistry,
        diags: &mut Diags,
    ) -> bool {
        let mut result = true;
        let mut old_locations = Vec::new();
        let mut stable = false;
        let mut iter_count = 0;
        const MAX_ITERATIONS: usize = 100;

        diags.suppress_arith_err = true;
        while result && !stable {
            self.trace(format_args!(
                "LayoutPhase::iterate: Iteration count {}",
                iter_count
            ));
            iter_count += 1;
            diags.trace_iteration = iter_count;
            trace!("LayoutPhase::iterate Beginning iteration {}", iter_count);

            result = self.run_one_pass(irdb, region_db, ext_registry, diags);

            if self.ir_locs == old_locations {
                stable = true;
            } else {
                if iter_count >= MAX_ITERATIONS {
                    let mut diff_i = 0;
                    for (i, (current, old)) in
                        self.ir_locs.iter().zip(old_locations.iter()).enumerate()
                    {
                        if current != old {
                            diff_i = i;
                            break;
                        }
                    }
                    let culprit_idx = diff_i.saturating_sub(1);
                    let msg = "Cyclic dependency detected: layout failed to stabilize after maximum iterations.";
                    let src_loc = irdb.ir_vec[culprit_idx].src_loc.clone();
                    diags.err1("ERR_179", msg, src_loc);
                    diags.suppress_arith_err = false;
                    return false;
                }
                old_locations = self.ir_locs.clone();
            }
        }

        // Final strict pass: confirm converged values produce no arithmetic errors.
        if result {
            diags.suppress_arith_err = false;
            iter_count += 1;
            diags.trace_iteration = iter_count;
            result = self.run_one_pass(irdb, region_db, ext_registry, diags);
        }

        diags.suppress_arith_err = false;
        result
    }
}
