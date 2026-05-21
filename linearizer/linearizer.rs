// Shared expression linearization for firmion.
//
// This crate provides the core types and expression-lowering logic shared by
// both the const-time (ConstDb) and layout-time (LayoutDb) pipeline stages.
// The crate carries no knowledge of const vs. layout context.  Each caller
// owns a Linearizer instance and the resulting IR/operand vectors.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::depth_guard::{DepthGuard, MAX_RECURSION_DEPTH};
use diags::Diags;
use diags::SourceSpan;
use indextree::NodeId;

#[allow(unused_imports)]
use tracing::debug;

use ast::{Ast, LexToken, TokenInfo};
use ir::{DataType, IRKind};
use symtable::SymbolTable;

/// An operand in the linearized IR. Two design choices greatly simplify lifetime
/// management:
/// * Callers hold indices into the Linearizer's operand_vec, not direct reference
///   to these structs.
/// * LinOperand owns its own data.
///
/// Four variants cover all operand roles:
///   * Literal: direct-value tokens (integers, strings).  The tok field
///     identifies the type; sval holds the source text.  Never substituted.
///   * Ref: an identifier value reference (e.g. a const name).
///     process_lin_operands replaces the operand with the const's typed value
///     if found in the symbol table; otherwise the operand becomes
///     ParameterValue::DeferredRef.
///   * NameDef: a structural identifier whose string IS the final value (extension
///     names, label names, const LHS, assign LHS).  Never substituted.
///   * Output: the result slot of a prior IR instruction.
pub enum LinOperand {
    /// Direct-value token: integer literal, string literal, etc.
    Literal {
        /// Source location for diagnostics.
        src_loc: SourceSpan,
        /// Token kind, e.g. U64, QuotedString.  Never Identifier or Label.
        tok: LexToken,
        /// Token text as parsed from source.
        sval: String,
        /// Named-argument parameter name, if the call site used `name=value` syntax.
        param_name: Option<String>,
        /// Data type established during lowering (or explicitly provided)
        data_type: DataType,
    },
    /// Identifier value reference; const-substitution applies if the identifier names a const.
    Ref {
        /// Source location for diagnostics.
        src_loc: SourceSpan,
        /// The identifier text to look up in the symbol table.
        sval: String,
        /// Named-argument parameter name, if the call site used `name=value` syntax.
        param_name: Option<String>,
        /// Data type established during lowering (or explicitly provided)
        data_type: DataType,
    },
    /// Identifier at a definition site: the string is the value, not a lookup key.
    /// Covers extension call names, label declarations, const LHS names, and assign LHS names.
    /// The same identifier string in an expression (e.g. addr(my_label)) becomes Ref.
    /// NameDef operands are never const-substituted and carry no param_name.
    NameDef {
        /// Source location for diagnostics.
        src_loc: SourceSpan,
        /// The identifier string used directly as the operand value.
        sval: String,
    },
    /// Result slot produced by a prior IR instruction.
    Output {
        /// Source location for diagnostics.
        src_loc: SourceSpan,
        /// Index of this output in the owning Linearizer's ir_vec.
        ir_lid: usize,
        /// Named-argument parameter name, if the call site used `name=value` syntax.
        /// None for positional arguments and for non-named-arg output placeholders.
        param_name: Option<String>,
        /// Data type of the output computed by the instruction
        data_type: DataType,
    },
}

impl LinOperand {
    /// Construct a direct-value literal operand from a token.
    pub fn new_literal(tinfo: &TokenInfo<'_>, data_type: DataType) -> Self {
        LinOperand::Literal {
            src_loc: tinfo.loc.clone(),
            sval: tinfo.val.to_string(),
            tok: tinfo.tok,
            param_name: None,
            data_type,
        }
    }

    /// Construct a value-reference operand for an identifier that may name a const.
    pub fn new_ref(tinfo: &TokenInfo<'_>, data_type: DataType) -> Self {
        LinOperand::Ref {
            src_loc: tinfo.loc.clone(),
            sval: tinfo.val.to_string(),
            param_name: None,
            data_type,
        }
    }

    /// Construct a definition-site name operand from a token.
    pub fn new_name(tinfo: &TokenInfo<'_>) -> Self {
        LinOperand::NameDef {
            src_loc: tinfo.loc.clone(),
            sval: tinfo.val.to_string(),
        }
    }

    /// Construct a definition-site name operand from a pre-built string (e.g. "ns::ext").
    pub fn new_name_str(sval: String, src_loc: SourceSpan) -> Self {
        LinOperand::NameDef { src_loc, sval }
    }

    /// Construct an output-slot operand for a computed IR result.
    pub fn new_output(ir_lid: usize, src_loc: SourceSpan, data_type: DataType) -> Self {
        LinOperand::Output {
            src_loc,
            ir_lid,
            param_name: None,
            data_type,
        }
    }

    /// Tag the operand with a named-argument parameter name.
    /// Panics if called on a NameDef operand, which cannot be a named-arg value.
    pub fn set_param_name(&mut self, name: String) {
        match self {
            LinOperand::Literal { param_name, .. } => *param_name = Some(name),
            LinOperand::Ref { param_name, .. } => *param_name = Some(name),
            LinOperand::Output { param_name, .. } => *param_name = Some(name),
            LinOperand::NameDef { .. } => panic!(
                "set_param_name called on a NameDef operand — NameDef operands are not named-arg values"
            ),
        }
    }

    /// Return the named-argument parameter name, if any.
    pub fn param_name(&self) -> Option<&str> {
        match self {
            LinOperand::Literal { param_name, .. } => param_name.as_deref(),
            LinOperand::Ref { param_name, .. } => param_name.as_deref(),
            LinOperand::Output { param_name, .. } => param_name.as_deref(),
            LinOperand::NameDef { .. } => None,
        }
    }

    /// Return the DataType established for this operand.
    pub fn data_type(&self) -> DataType {
        match self {
            LinOperand::Literal { data_type, .. } => *data_type,
            LinOperand::Ref { data_type, .. } => *data_type,
            LinOperand::Output { data_type, .. } => *data_type,
            LinOperand::NameDef { .. } => DataType::Identifier,
        }
    }
}

/// A single instruction in the linearized IR.
pub struct LinIR {
    /// AST node that generated this instruction.
    pub nid: NodeId,
    /// Source location for diagnostics.
    pub src_loc: SourceSpan,
    /// Operation kind.
    pub op: IRKind,
    /// Indices into the owning Linearizer's operand_vec.
    pub operand_vec: Vec<usize>,
}

impl LinIR {
    /// Construct a LinIR from an AST node and operation kind.
    pub fn new(nid: NodeId, ast: &Ast<'_>, op: IRKind) -> Self {
        let tinfo = ast.get_tinfo(nid);
        let src_loc = tinfo.loc.clone();
        Self {
            nid,
            src_loc,
            op,
            operand_vec: Vec::new(),
        }
    }

    /// Append an operand index to this instruction's operand list.
    pub fn add_operand(&mut self, operand_num: usize) {
        self.operand_vec.push(operand_num);
    }
}

/// Map a LexToken to the corresponding IRKind.  This function maps only
/// tokens that appear inside expressions or as IR opcodes.  Passing a
/// statement-level token (e.g. LexToken::Section or LexToken::Output) that
/// has no IRKind equivalent causes a panic.
pub fn tok_to_irkind(tok: LexToken) -> IRKind {
    match tok {
        LexToken::Addr => IRKind::Addr,
        LexToken::Align => IRKind::Align,
        LexToken::Ampersand => IRKind::BitAnd,
        LexToken::Assert => IRKind::Assert,
        LexToken::Asterisk => IRKind::Multiply,
        LexToken::Const => IRKind::Const,
        LexToken::DoubleAmpersand => IRKind::LogicalAnd,
        LexToken::DoubleEq => IRKind::DoubleEq,
        LexToken::DoubleGreater => IRKind::RightShift,
        LexToken::DoubleLess => IRKind::LeftShift,
        LexToken::DoublePipe => IRKind::LogicalOr,
        LexToken::Eq => IRKind::Eq,
        LexToken::FSlash => IRKind::Divide,
        LexToken::GEq => IRKind::GEq,
        LexToken::Gt => IRKind::Gt,
        LexToken::AddrOffset => IRKind::AddrOffset,
        LexToken::LEq => IRKind::LEq,
        LexToken::Lt => IRKind::Lt,
        LexToken::Minus => IRKind::Subtract,
        LexToken::NEq => IRKind::NEq,
        LexToken::Percent => IRKind::Modulo,
        LexToken::Pipe => IRKind::BitOr,
        LexToken::Plus => IRKind::Add,
        LexToken::Print => IRKind::Print,
        LexToken::Trace => IRKind::Trace,
        LexToken::SecOffset => IRKind::SecOffset,
        LexToken::FileOffset => IRKind::FileOffset,
        LexToken::SetAddr => IRKind::SetAddr,
        LexToken::PadAddrOffset => IRKind::SetAddrOffset,
        LexToken::PadSecOffset => IRKind::SetSecOffset,
        LexToken::PadFileOffset => IRKind::SetFileOffset,
        LexToken::ToI64 => IRKind::ToI64,
        LexToken::ToU64 => IRKind::ToU64,
        LexToken::Wr8 => IRKind::Wr(1, false),
        LexToken::Wr16 => IRKind::Wr(2, false),
        LexToken::Wr24 => IRKind::Wr(3, false),
        LexToken::Wr32 => IRKind::Wr(4, false),
        LexToken::Wr40 => IRKind::Wr(5, false),
        LexToken::Wr48 => IRKind::Wr(6, false),
        LexToken::Wr56 => IRKind::Wr(7, false),
        LexToken::Wr64 => IRKind::Wr(8, false),
        LexToken::Wrbe8 => IRKind::Wr(1, true),
        LexToken::Wrbe16 => IRKind::Wr(2, true),
        LexToken::Wrbe24 => IRKind::Wr(3, true),
        LexToken::Wrbe32 => IRKind::Wr(4, true),
        LexToken::Wrbe40 => IRKind::Wr(5, true),
        LexToken::Wrbe48 => IRKind::Wr(6, true),
        LexToken::Wrbe56 => IRKind::Wr(7, true),
        LexToken::Wrbe64 => IRKind::Wr(8, true),
        LexToken::ObjAlign => IRKind::ObjAlign,
        LexToken::ObjLma => IRKind::ObjLma,
        LexToken::ObjVma => IRKind::ObjVma,
        LexToken::Wrf => IRKind::Wrf,
        LexToken::Wrs => IRKind::Wrs,
        LexToken::BuiltinOutputSize => IRKind::BuiltinOutputSize,
        LexToken::BuiltinOutputAddr => IRKind::BuiltinOutputAddr,
        LexToken::BuiltinVersionString => IRKind::BuiltinVersionString,
        LexToken::BuiltinVersionMajor => IRKind::BuiltinVersionMajor,
        LexToken::BuiltinVersionMinor => IRKind::BuiltinVersionMinor,
        LexToken::BuiltinVersionPatch => IRKind::BuiltinVersionPatch,
        bug => panic!("Failed to convert LexToken to IRKind for {:?}", bug),
    }
}

/// Owns a flat IR vector and operand vector built up during a single
/// linearization pass.  Has no lifetime parameters and owns all stored data.
/// Callers hold usize indices into the vectors rather than references to simplify
/// borrow checking.
pub struct Linearizer {
    pub ir_vec: Vec<LinIR>,
    pub operand_vec: Vec<LinOperand>,
}

impl Default for Linearizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Linearizer {
    /// Create an empty Linearizer with no IR or operands.
    pub fn new() -> Self {
        Self {
            ir_vec: Vec::new(),
            operand_vec: Vec::new(),
        }
    }

    /// Append a new IR entry and return its index (lid).
    pub fn new_ir(&mut self, nid: NodeId, ast: &Ast<'_>, op: IRKind) -> usize {
        let lir = LinIR::new(nid, ast, op);
        let lid = self.ir_vec.len();
        self.ir_vec.push(lir);
        lid
    }

    /// Allocate and initialize a new operand in the operand vector, then
    /// append the operand's index to the IR's own operand vector.
    pub fn add_new_operand_to_ir(&mut self, ir_lid: usize, operand: LinOperand) -> usize {
        let idx = self.operand_vec.len();
        self.operand_vec.push(operand);
        self.ir_vec[ir_lid].add_operand(idx);
        idx
    }

    /// Append an existing operand's index to the given IR.
    pub fn add_existing_operand_to_ir(&mut self, ir_lid: usize, idx: usize) {
        self.ir_vec[ir_lid].add_operand(idx);
    }

    /// Return false and emit an error if this IR does not have exactly the
    /// expected number of operands.
    pub fn operand_count_is_valid(
        &self,
        expected: usize,
        lops: &[usize],
        diags: &mut Diags,
        tinfo: &TokenInfo<'_>,
    ) -> bool {
        let found = lops.len();
        if found != expected {
            diags.err1(
                "ERR_204",
                &format!(
                    "Expected {} operand(s), but found {} for '{}' expression",
                    expected, found, tinfo.val
                ),
                tinfo.span(),
            );
            return false;
        }
        true
    }

    /// Validate that the number of operands for this IR is correct then append
    /// them to the IR's operand vector.  Return false if validation fails.
    pub fn process_operands(
        &mut self,
        expected: usize,
        lops: &mut Vec<usize>,
        ir_lid: usize,
        diags: &mut Diags,
        tinfo: &TokenInfo<'_>,
    ) -> bool {
        if self.operand_count_is_valid(expected, lops, diags, tinfo) {
            for idx in lops {
                self.add_existing_operand_to_ir(ir_lid, *idx);
            }
            true
        } else {
            false
        }
    }

    /// Like `process_operands`, but succeed silently when we have no operands.
    /// Used for expressions with optional operands such as `addr()`.
    pub fn process_optional_operands(
        &mut self,
        expected: usize,
        lops: &mut Vec<usize>,
        ir_lid: usize,
        diags: &mut Diags,
        tinfo: &TokenInfo<'_>,
    ) -> bool {
        if lops.is_empty() {
            return true;
        }
        self.process_operands(expected, lops, ir_lid, diags, tinfo)
    }

    /// Recurse over each AST child of the specified AST node and call
    /// record_expr_r.  We accumulating result operand indices in the `lops`
    /// operand vector.
    pub fn record_expr_children_r(
        &mut self,
        parent_nid: NodeId,
        lops: &mut Vec<usize>,
        symbol_table: &SymbolTable,
        diags: &mut Diags,
        ast: &Ast<'_>,
    ) -> bool {
        let mut result = true;
        for nid in ast.children(parent_nid) {
            result &= self.record_expr_r(nid, lops, symbol_table, diags, ast);
        }
        result
    }

    /// Recursively lower one expression AST node into IR/operand entries.
    ///
    /// Handles all expression tokens: literals, identifiers, binary operators,
    /// type conversions, builtin variables, and extension calls.  Caller must
    /// not pass statement tokens (wr32, section, align, etc.).
    ///
    /// Returns true on success with operand indices appended to
    /// returned_operands.
    pub fn record_expr_r(
        &mut self,
        parent_nid: NodeId,
        returned_operands: &mut Vec<usize>,
        symbol_table: &SymbolTable,
        diags: &mut Diags,
        ast: &Ast<'_>,
    ) -> bool {
        debug!("Linearizer::record_expr_r: ENTER for nid: {}", parent_nid);
        let Some(_guard) = DepthGuard::enter(MAX_RECURSION_DEPTH) else {
            let tinfo = ast.get_tinfo(parent_nid);
            diags.err1(
                "ERR_203",
                &format!(
                    "Maximum recursion depth ({MAX_RECURSION_DEPTH}) exceeded when processing '{}'.",
                    tinfo.val
                ),
                tinfo.span(),
            );
            return false;
        };

        let tinfo = ast.get_tinfo(parent_nid);
        let tok = tinfo.tok;
        let mut result = true;

        match tok {
            LexToken::U64 | LexToken::I64 | LexToken::Integer | LexToken::QuotedString => {
                let dt = match tok {
                    LexToken::U64 => DataType::U64,
                    LexToken::I64 => DataType::I64,
                    LexToken::Integer => DataType::Integer,
                    LexToken::QuotedString => DataType::QuotedString,
                    _ => unreachable!(),
                };
                let idx = self.operand_vec.len();
                self.operand_vec.push(LinOperand::new_literal(tinfo, dt));
                returned_operands.push(idx);
            }

            LexToken::Identifier => {
                // parent_nid is an identifier.
                if ast.has_children(parent_nid) {
                    // An identifier with children means this is an extension call.
                    let ir_lid = self.new_ir(parent_nid, ast, IRKind::ExtensionCall);
                    // Add the extension's name as the first operand so the
                    // backend can look up the extension.  Name variant: the
                    // linearizer never const-substitutes the name.
                    self.add_new_operand_to_ir(ir_lid, LinOperand::new_name(tinfo));

                    // Now recursively add operands for the extension call arguments.
                    let mut lops = Vec::new();
                    for child in ast.children(parent_nid) {
                        result &= self.record_expr_r(child, &mut lops, symbol_table, diags, ast);
                    }
                    for idx in lops {
                        self.add_existing_operand_to_ir(ir_lid, idx);
                    }

                    // Output operand for the extension result.  DataType::Extension
                    // causes type checking to reject use in arithmetic, wr8..64,
                    // wrs, or const expressions.
                    let out_idx = self.add_new_operand_to_ir(
                        ir_lid,
                        LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::Extension),
                    );
                    returned_operands.push(out_idx);
                } else {
                    // Leaf identifier — const ref or section/label name used as value.
                    // process_lin_operands substitutes the operand if the identifier names a const.
                    let dt = symbol_table
                        .get_value(tinfo.val)
                        .map(|v| v.data_type())
                        .unwrap_or(DataType::DeferredRef);
                    let idx = self.operand_vec.len();
                    self.operand_vec.push(LinOperand::new_ref(tinfo, dt));
                    returned_operands.push(idx);
                }
            }

            // Namespace extension call: custom::foo(args…)
            LexToken::Namespace => {
                let mut children = ast.children(parent_nid);
                let id_child = children.next().unwrap();
                let id_tinfo = ast.get_tinfo(id_child);
                let extension_name = format!("{}{}", tinfo.val, id_tinfo.val);
                let ir_lid = self.new_ir(parent_nid, ast, IRKind::ExtensionCall);
                // Store the full qualified name (e.g. "custom::foo") as a Name
                // operand so the backend can look it up without const substitution.
                self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_name_str(extension_name, tinfo.loc.clone()),
                );
                let mut lops = Vec::new();
                for child in children {
                    result &= self.record_expr_r(child, &mut lops, symbol_table, diags, ast);
                }
                for idx in lops {
                    self.add_existing_operand_to_ir(ir_lid, idx);
                }
                // Output operand: same role as in the Identifier arm above.
                let out_idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::Extension),
                );
                returned_operands.push(out_idx);
            }

            // Built-in values have exactly one output operand.
            LexToken::BuiltinOutputSize
            | LexToken::BuiltinOutputAddr
            | LexToken::BuiltinVersionMajor
            | LexToken::BuiltinVersionMinor
            | LexToken::BuiltinVersionPatch => {
                let ir_lid = self.new_ir(parent_nid, ast, tok_to_irkind(tok));
                let idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::U64),
                );
                returned_operands.push(idx);
            }
            LexToken::BuiltinVersionString => {
                let ir_lid = self.new_ir(parent_nid, ast, tok_to_irkind(tok));
                let idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::QuotedString),
                );
                returned_operands.push(idx);
            }

            LexToken::ToI64 => {
                let mut lops = Vec::new();
                result &=
                    self.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                let ir_lid = self.new_ir(parent_nid, ast, tok_to_irkind(tok));
                result &= self.process_operands(1, &mut lops, ir_lid, diags, tinfo);
                let idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::I64),
                );
                returned_operands.push(idx);
            }
            LexToken::ToU64 => {
                let mut lops = Vec::new();
                result &=
                    self.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                let ir_lid = self.new_ir(parent_nid, ast, tok_to_irkind(tok));
                result &= self.process_operands(1, &mut lops, ir_lid, diags, tinfo);
                let idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::U64),
                );
                returned_operands.push(idx);
            }

            LexToken::Eq
            | LexToken::NEq
            | LexToken::LEq
            | LexToken::GEq
            | LexToken::Lt
            | LexToken::Gt
            | LexToken::DoubleEq
            | LexToken::DoublePipe
            | LexToken::DoubleAmpersand => {
                let mut lops = Vec::new();
                result &=
                    self.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                let ir_lid = self.new_ir(parent_nid, ast, tok_to_irkind(tok));
                result &= self.process_operands(2, &mut lops, ir_lid, diags, tinfo);

                if lops.len() == 2 {
                    let lhs_dt = self.operand_vec[lops[0]].data_type();
                    let rhs_dt = self.operand_vec[lops[1]].data_type();
                    let mut ok = false;
                    if lhs_dt == rhs_dt {
                        if [
                            DataType::I64,
                            DataType::U64,
                            DataType::Integer,
                            DataType::QuotedString,
                            DataType::Identifier,
                            DataType::DeferredRef,
                        ]
                        .contains(&lhs_dt)
                        {
                            ok = true;
                        }
                    } else if (rhs_dt == DataType::Integer
                        && [DataType::I64, DataType::U64].contains(&lhs_dt))
                        || (lhs_dt == DataType::Integer
                            && [DataType::I64, DataType::U64].contains(&rhs_dt))
                    {
                        ok = true;
                    } else if lhs_dt == DataType::DeferredRef || rhs_dt == DataType::DeferredRef {
                        ok = true; // Resolve at layout time
                    }
                    if !ok {
                        let msg = format!(
                            "Type mismatch in comparison: '{:?}' and '{:?}'.",
                            lhs_dt, rhs_dt
                        );
                        diags.err1("ERR_207", &msg, tinfo.loc.clone());
                        result = false;
                    }
                }

                let idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::U64),
                );
                returned_operands.push(idx);
            }

            LexToken::DoubleGreater
            | LexToken::DoubleLess
            | LexToken::Asterisk
            | LexToken::Ampersand
            | LexToken::Pipe
            | LexToken::FSlash
            | LexToken::Percent
            | LexToken::Minus
            | LexToken::Plus => {
                let mut lops = Vec::new();
                result &=
                    self.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                let ir_lid = self.new_ir(parent_nid, ast, tok_to_irkind(tok));
                result &= self.process_operands(2, &mut lops, ir_lid, diags, tinfo);

                let mut out_dt = DataType::Unknown;
                if lops.len() == 2 {
                    let lhs_dt = self.operand_vec[lops[0]].data_type();
                    let rhs_dt = self.operand_vec[lops[1]].data_type();

                    if lhs_dt == rhs_dt {
                        if [DataType::I64, DataType::U64, DataType::Integer].contains(&lhs_dt) {
                            out_dt = lhs_dt;
                        } else if lhs_dt == DataType::DeferredRef {
                            out_dt = DataType::DeferredRef; // Resolve at layout time
                        } else {
                            let msg = format!(
                                "Invalid operand types for arithmetic: '{:?}' and '{:?}'.",
                                lhs_dt, rhs_dt
                            );
                            diags.err1("ERR_205", &msg, tinfo.loc.clone());
                            result = false;
                        }
                    } else if rhs_dt == DataType::Integer
                        && [DataType::I64, DataType::U64].contains(&lhs_dt)
                    {
                        out_dt = lhs_dt;
                    } else if lhs_dt == DataType::Integer
                        && [DataType::I64, DataType::U64].contains(&rhs_dt)
                    {
                        out_dt = rhs_dt;
                    } else if lhs_dt == DataType::DeferredRef || rhs_dt == DataType::DeferredRef {
                        // When one operand is a DeferredRef and the other is a
                        // numeric type, propagate the numeric type.  If both are
                        // DeferredRefs, keep DeferredRef for layout-time resolution.
                        if [DataType::I64, DataType::U64, DataType::Integer].contains(&lhs_dt) {
                            out_dt = lhs_dt;
                        } else if [DataType::I64, DataType::U64, DataType::Integer]
                            .contains(&rhs_dt)
                        {
                            out_dt = rhs_dt;
                        } else {
                            out_dt = DataType::DeferredRef;
                        }
                    } else {
                        let msg = format!(
                            "Type mismatch in arithmetic: '{:?}' and '{:?}'.",
                            lhs_dt, rhs_dt
                        );
                        diags.err1("ERR_206", &msg, tinfo.loc.clone());
                        result = false;
                    }
                }

                let idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), out_dt),
                );
                returned_operands.push(idx);
            }

            // ── sizeof(section) or sizeof(namespace::ext) ─────────────────
            LexToken::Sizeof => {
                let first_child = ast.children(parent_nid).next().unwrap();
                let first_child_tinfo = ast.get_tinfo(first_child);

                if first_child_tinfo.tok == LexToken::Namespace {
                    let ns_children: Vec<_> = ast.children(first_child).collect();
                    let ext_id_tinfo = ast.get_tinfo(ns_children[0]);
                    let full_name = format!("{}{}", first_child_tinfo.val, ext_id_tinfo.val);
                    let ir_lid = self.new_ir(parent_nid, ast, IRKind::SizeofExt);
                    self.add_new_operand_to_ir(
                        ir_lid,
                        LinOperand::new_name_str(full_name, first_child_tinfo.loc.clone()),
                    );
                    let idx = self.add_new_operand_to_ir(
                        ir_lid,
                        LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::U64),
                    );
                    returned_operands.push(idx);
                } else {
                    let mut lops = Vec::new();
                    let ir_lid = self.new_ir(parent_nid, ast, IRKind::Sizeof);
                    result &= self.record_expr_children_r(
                        parent_nid,
                        &mut lops,
                        symbol_table,
                        diags,
                        ast,
                    );
                    result &= self.process_operands(1, &mut lops, ir_lid, diags, tinfo);
                    let idx = self.add_new_operand_to_ir(
                        ir_lid,
                        LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::U64),
                    );
                    returned_operands.push(idx);
                }
            }

            // ── obj_align/obj_lma/obj_vma: one required identifier, one output ─
            LexToken::ObjAlign | LexToken::ObjLma | LexToken::ObjVma => {
                let mut lops = Vec::new();
                let ir_lid = self.new_ir(parent_nid, ast, tok_to_irkind(tok));
                result &=
                    self.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                result &= self.process_operands(1, &mut lops, ir_lid, diags, tinfo);
                let idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::U64),
                );
                returned_operands.push(idx);
            }

            // ── Address queries: addr([label]), addr_offset([label]), etc. ──
            LexToken::Addr | LexToken::AddrOffset | LexToken::SecOffset | LexToken::FileOffset => {
                let mut lops = Vec::new();
                let ir_lid = self.new_ir(parent_nid, ast, tok_to_irkind(tok));
                result &=
                    self.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                result &= self.process_optional_operands(1, &mut lops, ir_lid, diags, tinfo);
                let idx = self.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), DataType::U64),
                );
                returned_operands.push(idx);
            }

            // Syntactic stuff, no IR emitted.
            LexToken::Semicolon
            | LexToken::Comma
            | LexToken::OpenParen
            | LexToken::CloseParen
            | LexToken::OpenBrace
            | LexToken::CloseBrace
            // RegionRef is a structural annotation on a section node; not a
            // layout statement and never reaches expression evaluation.
            | LexToken::RegionRef => {}

            // Named argument: name=expr -------------------------------------
            // A NamedArg node (synthesized by the parser) wraps one RHS expression
            // child.  Lower the child normally, then tag the resulting operand(s)
            // with the parameter name so IRDb can reorder to declaration order.
            LexToken::NamedArg => {
                let param_name = tinfo.val.to_string();
                let before = returned_operands.len();
                let child = ast.children(parent_nid).next().unwrap();
                result &= self.record_expr_r(child, returned_operands, symbol_table, diags, ast);
                // Tag every operand added for this arg with the parameter name.
                for idx in &returned_operands[before..] {
                    self.operand_vec[*idx].set_param_name(param_name.clone());
                }
            }

            // ── Anything else is a bug: statement tokens must not reach here
            _ => {
                let msg = format!("'{}' is not valid in an expression context", tinfo.val);
                diags.err1("ERR_214", &msg, tinfo.span());
                result = false;
            }
        }

        debug!(
            "Linearizer::record_expr_r: EXIT({}) for nid: {}",
            result, parent_nid
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linearizer_default() {
        let lz = Linearizer::default();
        assert_eq!(lz.ir_vec.len(), 0);
        assert_eq!(lz.operand_vec.len(), 0);
    }
}
