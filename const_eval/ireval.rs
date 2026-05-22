// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::Diags;
use ir::{DataType, IR, IROperand, ParameterValue};

pub struct ParmValDb {
    pub parms: Vec<ParameterValue>,
}

impl ParmValDb {
    pub fn new(parms: Vec<ParameterValue>) -> Self {
        Self { parms }
    }
}

/// Build a string from a sequence of typed operand values.
/// `parms` and `ir_parms` are global operand tables shared across all IR instructions.
/// Each entry has a unique integer ID assigned at IR construction time.
/// `indices` is `ir.operands`: the list of IDs that belong to this one instruction,
/// used to select the relevant entries from the global tables.
pub fn evaluate_string_expr(
    parms: &[ParameterValue],
    ir_parms: &[IROperand],
    indices: &[usize],
    diags: &mut Diags,
) -> Option<String> {
    let mut result = true;
    let mut xstr = String::new();
    for &op_num in indices {
        let op = &parms[op_num];
        match op.data_type() {
            DataType::QuotedString => xstr.push_str(op.to_str()),
            DataType::U64 => xstr.push_str(&format!("{:#X}", op.to_u64())),
            DataType::Integer | DataType::I64 => xstr.push_str(&format!("{}", op.to_i64())),
            bad => {
                let msg = format!("Cannot stringify type '{:?}'", bad);
                diags.err1("ERR_138", &msg, ir_parms[op_num].src_loc.clone());
                result = false;
            }
        }
    }
    if result { Some(xstr) } else { None }
}

/// Evaluate one assert instruction.  Returns true if the assert passed.
/// On failure, emits ERR_126 and backtrack notes, then returns false.
pub fn execute_assert(
    parms: &[ParameterValue],
    ir_parms: &[IROperand],
    ir_vec: &[IR],
    ir: &IR,
    diags: &mut Diags,
) -> bool {
    let opnd_num = ir.operands[0];
    if !parms[opnd_num]
        .to_bool()
        .expect("assert operand must be numeric; IRDb type check failed")
    {
        diags.err1("ERR_126", "Assert expression failed", ir.src_loc.clone());
        assert_info(parms, ir_parms, ir_vec, opnd_num, diags);
        return false;
    }
    true
}

fn assert_info(
    parms: &[ParameterValue],
    ir_parms: &[IROperand],
    ir_vec: &[IR],
    opnd_num: usize,
    diags: &mut Diags,
) {
    let Some(src_lid) = ir_parms[opnd_num].ir_lid else {
        return;
    };
    let operation = &ir_vec[src_lid];
    let num_operands = operation.operands.len();
    for (idx, &op) in operation.operands.iter().enumerate() {
        if idx < num_operands - 1 && parms[op].data_type() == DataType::U64 {
            let msg = format!("Operand has value {}", parms[op].to_u64());
            diags.note1("ERR_132", &msg, ir_parms[op].src_loc.clone());
        }
    }
}
