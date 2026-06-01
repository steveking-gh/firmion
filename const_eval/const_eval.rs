// Const-time evaluation for firmion.
//
// The public interface is `evaluate_and_prune()`, which:
//   1. Walks the immutable AST to evaluate constants, if-conditions, and asserts.
//   2. Clones the AST and prunes all `if/else` nodes.
//
// The caller receives a fully resolved `SymbolTable` and a strictly immutable `Ast`
// ready for the LayoutDb phase.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

pub mod ireval;
pub mod linearizer;

use anyhow::bail;
use diags::depth_guard::{DepthGuard, MAX_RECURSION_DEPTH};
use diags::{Diags, SourceSpan};
use indextree::NodeId;
use parse_int::parse;
use std::collections::HashMap;

#[allow(unused_imports)]
use tracing::{debug, trace};

use ast::{Ast, LexToken};
use ast::astdb::AstDb;
use ir::{ConstBuiltins, ObjProps, ParameterValue, RegionProps, strip_kmg};
use ir::symtable::SymbolTable;

// ── Internal error type for const arithmetic ─────────────────────────────────

enum CalcErr {
    Overflow(String),
    DivByZero,
}

// ── Expression Evaluator (Tree Walker) ─────────────────────────────────────────

/// Evaluate an expression subtree natively on the AST.
pub fn eval_expr_tree(
    ast: &Ast,
    nid: NodeId,
    symbol_table: &mut SymbolTable,
    diags: &mut Diags,
) -> Option<ParameterValue> {
    let _guard = DepthGuard::enter(MAX_RECURSION_DEPTH).or_else(|| {
        diags.err1(
            "ERR_115",
            &format!(
                "Const expression nesting depth exceeds maximum ({}).",
                MAX_RECURSION_DEPTH
            ),
            ast.get_tinfo(nid).loc.clone(),
        );
        None
    })?;

    let tinfo = ast.get_tinfo(nid);
    let src_loc = &tinfo.loc;

    match tinfo.tok {
        // --- Literals ---
        LexToken::Integer => {
            let (base, mult) = strip_kmg(tinfo.val);
            let v: i64 = parse::<i64>(base)
                .ok()
                .and_then(|v| v.checked_mul(mult as i64))
                .ok_or(())
                .ok()
                .or_else(|| {
                    diags.err1(
                        "ERR_88",
                        &format!("Malformed integer in const expression: {}", tinfo.val),
                        src_loc.clone(),
                    );
                    None
                })?;
            Some(ParameterValue::Integer(v))
        }
        LexToken::U64 => {
            let no_u = tinfo.val.strip_suffix('u').unwrap_or(tinfo.val);
            let (base, mult) = strip_kmg(no_u);
            let v: u64 = parse::<u64>(base)
                .ok()
                .and_then(|v| v.checked_mul(mult))
                .ok_or(())
                .ok()
                .or_else(|| {
                    diags.err1(
                        "ERR_89",
                        &format!("Malformed U64 in const expression: {}", tinfo.val),
                        src_loc.clone(),
                    );
                    None
                })?;
            Some(ParameterValue::U64(v))
        }
        LexToken::I64 => {
            let no_i = tinfo.val.strip_suffix('i').unwrap_or(tinfo.val);
            let (base, mult) = strip_kmg(no_i);
            let v: i64 = parse::<i64>(base)
                .ok()
                .and_then(|v| v.checked_mul(mult as i64))
                .ok_or(())
                .ok()
                .or_else(|| {
                    diags.err1(
                        "ERR_90",
                        &format!("Malformed I64 in const expression: {}", tinfo.val),
                        src_loc.clone(),
                    );
                    None
                })?;
            Some(ParameterValue::I64(v))
        }
        LexToken::QuotedString => {
            let trimmed = tinfo
                .val
                .strip_prefix('"')
                .unwrap_or(tinfo.val)
                .strip_suffix('"')
                .unwrap_or(tinfo.val);
            let unescaped = ir::unescape_string(trimmed);
            Some(ParameterValue::QuotedString(unescaped))
        }
        LexToken::Identifier => {
            let name = tinfo.val.to_string();
            if let Some(val) = symbol_table.get_value(&name) {
                symbol_table.mark_used(&name);
                Some(val)
            } else {
                diags.err1("ERR_86", &format!("Unknown or uninitialized identifier '{}' in const expression. Constants must be defined before use.", name), src_loc.clone());
                None
            }
        }

        // --- Builtins ---
        LexToken::BuiltinVersionString => Some(ParameterValue::QuotedString(
            ConstBuiltins::get().firmion_version_string.to_string(),
        )),
        LexToken::BuiltinVersionMajor => Some(ParameterValue::U64(
            ConstBuiltins::get().firmion_version_major,
        )),
        LexToken::BuiltinVersionMinor => Some(ParameterValue::U64(
            ConstBuiltins::get().firmion_version_minor,
        )),
        LexToken::BuiltinVersionPatch => Some(ParameterValue::U64(
            ConstBuiltins::get().firmion_version_patch,
        )),

        // --- Unary ---
        LexToken::ToI64 | LexToken::ToU64 => {
            let child_nid = ast.children(nid).next().unwrap();
            let val = eval_expr_tree(ast, child_nid, symbol_table, diags)?;
            match (&val, tinfo.tok) {
                (ParameterValue::U64(v), LexToken::ToI64) => Some(ParameterValue::I64(*v as i64)),
                (ParameterValue::I64(_) | ParameterValue::Integer(_), LexToken::ToI64) => {
                    Some(ParameterValue::I64(val.to_i64()))
                }
                (
                    ParameterValue::U64(_) | ParameterValue::I64(_) | ParameterValue::Integer(_),
                    LexToken::ToU64,
                ) => Some(ParameterValue::U64(val.to_u64())),
                _ => {
                    diags.err1(
                        "ERR_87",
                        &format!(
                            "Cannot apply '{:?}' to {:?} in a const expression.",
                            tinfo.tok,
                            val.data_type()
                        ),
                        src_loc.clone(),
                    );
                    None
                }
            }
        }

        // --- Binary ---
        LexToken::Plus
        | LexToken::Minus
        | LexToken::Asterisk
        | LexToken::FSlash
        | LexToken::Percent
        | LexToken::Ampersand
        | LexToken::Pipe
        | LexToken::DoubleLess
        | LexToken::DoubleGreater => {
            let mut it = ast.children(nid);
            let lhs_val = eval_expr_tree(ast, it.next().unwrap(), symbol_table, diags)?;
            let rhs_val = eval_expr_tree(ast, it.next().unwrap(), symbol_table, diags)?;
            apply_binary_op(tinfo.tok, lhs_val, rhs_val, src_loc, diags)
        }

        // --- Comparisons ---
        LexToken::DoubleEq
        | LexToken::NEq
        | LexToken::GEq
        | LexToken::LEq
        | LexToken::Gt
        | LexToken::Lt => {
            let mut it = ast.children(nid);
            let lhs_val = eval_expr_tree(ast, it.next().unwrap(), symbol_table, diags)?;
            let rhs_val = eval_expr_tree(ast, it.next().unwrap(), symbol_table, diags)?;
            apply_comparison_op(tinfo.tok, lhs_val, rhs_val, src_loc, diags)
        }

        // --- Logical ---
        LexToken::DoubleAmpersand | LexToken::DoublePipe => {
            let mut it = ast.children(nid);
            let lhs_val = eval_expr_tree(ast, it.next().unwrap(), symbol_table, diags)?;
            let rhs_val = eval_expr_tree(ast, it.next().unwrap(), symbol_table, diags)?;
            let Some(lhs_b) = lhs_val.to_bool() else {
                diags.err1(
                    "ERR_114",
                    "'&&'/'||' operands must be numeric",
                    src_loc.clone(),
                );
                return None;
            };
            let Some(rhs_b) = rhs_val.to_bool() else {
                diags.err1(
                    "ERR_124",
                    "'&&'/'||' operands must be numeric",
                    src_loc.clone(),
                );
                return None;
            };
            let result = if tinfo.tok == LexToken::DoubleAmpersand {
                lhs_b && rhs_b
            } else {
                lhs_b || rhs_b
            };
            Some(ParameterValue::U64(if result { 1 } else { 0 }))
        }

        // --- Rejected layout ops ---
        LexToken::Sizeof
        | LexToken::BuiltinOutputSize
        | LexToken::BuiltinOutputAddr
        | LexToken::Addr
        | LexToken::AddrOffset
        | LexToken::SecOffset
        | LexToken::FileOffset
        | LexToken::ObjAlign
        | LexToken::ObjLma
        | LexToken::ObjVma => {
            diags.err1("ERR_85", &format!("Operation '{:?}' cannot be used in a const expression because it requires engine-time layout or addressing.", tinfo.tok), src_loc.clone());
            None
        }

        _ => {
            diags.err1(
                "ERR_116",
                &format!(
                    "Operation '{:?}' is not supported in a const expression.",
                    tinfo.tok
                ),
                src_loc.clone(),
            );
            None
        }
    }
}

// ── Numeric helpers ────────────────────────────────────────────────────────────

fn coerce_numeric_pair(
    lhs: ParameterValue,
    rhs: ParameterValue,
    err_code: &str,
    src_loc: &SourceSpan,
    diags: &mut Diags,
) -> Option<(ParameterValue, ParameterValue)> {
    use ParameterValue::*;
    match (&lhs, &rhs) {
        (U64(_), U64(_))
        | (I64(_), I64(_))
        | (Integer(_), Integer(_))
        | (QuotedString(_), QuotedString(_)) => Some((lhs, rhs)),
        (U64(_), Integer(v)) => Some((lhs, U64(*v as u64))),
        (Integer(v), U64(_)) => Some((U64(*v as u64), rhs)),
        (I64(_), Integer(v)) => Some((lhs, I64(*v))),
        (Integer(v), I64(_)) => Some((I64(*v), rhs)),
        _ => {
            diags.err1(
                err_code,
                &format!(
                    "Type mismatch in const expression: {:?} and {:?}.",
                    lhs.data_type(),
                    rhs.data_type()
                ),
                src_loc.clone(),
            );
            None
        }
    }
}

fn apply_binary_op(
    tok: LexToken,
    lhs: ParameterValue,
    rhs: ParameterValue,
    src_loc: &SourceSpan,
    diags: &mut Diags,
) -> Option<ParameterValue> {
    use ParameterValue::*;
    let (lhs, rhs) = coerce_numeric_pair(lhs, rhs, "ERR_91", src_loc, diags)?;

    let emit = |err: CalcErr, diags: &mut Diags| -> Option<ParameterValue> {
        match err {
            CalcErr::Overflow(msg) => diags.err1("ERR_93", &msg, src_loc.clone()),
            CalcErr::DivByZero => diags.err1(
                "ERR_94",
                "Division by zero in const expression",
                src_loc.clone(),
            ),
        }
        None
    };

    match lhs {
        U64(a) => {
            let b = rhs.to_u64();
            match calc_u64_op(tok, a, b) {
                Ok(r) => Some(U64(r)),
                Err(e) => emit(e, diags),
            }
        }
        I64(a) => {
            let b = rhs.to_i64();
            match calc_i64_op(tok, a, b) {
                Ok(r) => Some(I64(r)),
                Err(e) => emit(e, diags),
            }
        }
        Integer(a) => {
            let b = rhs.to_i64();
            match calc_i64_op(tok, a, b) {
                Ok(r) => Some(Integer(r)),
                Err(e) => emit(e, diags),
            }
        }
        _ => {
            diags.err1(
                "ERR_92",
                &format!(
                    "Non-numeric type {:?} in arithmetic const expression.",
                    lhs.data_type()
                ),
                src_loc.clone(),
            );
            None
        }
    }
}

fn apply_comparison_op(
    tok: LexToken,
    lhs: ParameterValue,
    rhs: ParameterValue,
    src_loc: &SourceSpan,
    diags: &mut Diags,
) -> Option<ParameterValue> {
    use ParameterValue::*;
    if let (QuotedString(a), QuotedString(b)) = (&lhs, &rhs) {
        let result = match tok {
            LexToken::DoubleEq => a == b,
            LexToken::NEq => a != b,
            _ => {
                diags.err1(
                    "ERR_96",
                    "Ordered comparison (>=, <=) is not supported for strings.",
                    src_loc.clone(),
                );
                return None;
            }
        };
        return Some(U64(if result { 1 } else { 0 }));
    }
    let (lhs, rhs) = coerce_numeric_pair(lhs, rhs, "ERR_95", src_loc, diags)?;

    let result = match lhs {
        U64(a) => {
            let b = rhs.to_u64();
            match tok {
                LexToken::DoubleEq => a == b,
                LexToken::NEq => a != b,
                LexToken::GEq => a >= b,
                LexToken::LEq => a <= b,
                LexToken::Gt => a > b,
                LexToken::Lt => a < b,
                _ => unreachable!(),
            }
        }
        I64(a) | Integer(a) => {
            let b = rhs.to_i64();
            match tok {
                LexToken::DoubleEq => a == b,
                LexToken::NEq => a != b,
                LexToken::GEq => a >= b,
                LexToken::LEq => a <= b,
                LexToken::Gt => a > b,
                LexToken::Lt => a < b,
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    };
    Some(U64(if result { 1 } else { 0 }))
}

fn calc_u64_op(tok: LexToken, a: u64, b: u64) -> Result<u64, CalcErr> {
    match tok {
        LexToken::Plus => a.checked_add(b).ok_or_else(|| {
            CalcErr::Overflow(format!("Add expression '{a} + {b}' will overflow type U64"))
        }),
        LexToken::Minus => a.checked_sub(b).ok_or_else(|| {
            CalcErr::Overflow(format!(
                "Subtract expression '{a} - {b}' will underflow type U64"
            ))
        }),
        LexToken::Asterisk => a.checked_mul(b).ok_or_else(|| {
            CalcErr::Overflow(format!(
                "Multiply expression '{a} * {b}' will overflow type U64"
            ))
        }),
        LexToken::FSlash => a.checked_div(b).ok_or(CalcErr::DivByZero),
        LexToken::Percent => {
            if b == 0 {
                Err(CalcErr::DivByZero)
            } else {
                Ok(a % b)
            }
        }
        LexToken::Ampersand => Ok(a & b),
        LexToken::Pipe => Ok(a | b),
        LexToken::DoubleLess => Ok(a << (b & 63)),
        LexToken::DoubleGreater => Ok(a >> (b & 63)),
        _ => Err(CalcErr::Overflow(
            "Unknown operator in U64 const expression".to_string(),
        )),
    }
}

fn calc_i64_op(tok: LexToken, a: i64, b: i64) -> Result<i64, CalcErr> {
    match tok {
        LexToken::Plus => a.checked_add(b).ok_or_else(|| {
            CalcErr::Overflow(format!("Add expression '{a} + {b}' will overflow type I64"))
        }),
        LexToken::Minus => a.checked_sub(b).ok_or_else(|| {
            CalcErr::Overflow(format!(
                "Subtract expression '{a} - {b}' will underflow type I64"
            ))
        }),
        LexToken::Asterisk => a.checked_mul(b).ok_or_else(|| {
            CalcErr::Overflow(format!(
                "Multiply expression '{a} * {b}' will overflow type I64"
            ))
        }),
        LexToken::FSlash => {
            if b == 0 {
                Err(CalcErr::DivByZero)
            } else {
                Ok(a / b)
            }
        }
        LexToken::Percent => {
            if b == 0 {
                Err(CalcErr::DivByZero)
            } else {
                Ok(a % b)
            }
        }
        LexToken::Ampersand => Ok(a & b),
        LexToken::Pipe => Ok(a | b),
        LexToken::DoubleLess => Ok(a << (b & 63)),
        LexToken::DoubleGreater => Ok(a >> (b & 63)),
        _ => Err(CalcErr::Overflow(
            "Unknown operator in I64 const expression".to_string(),
        )),
    }
}

// ── Region Evaluator ──────────────────────────────────────────────────────────

pub fn evaluate_regions(
    diags: &mut Diags,
    ast: &Ast,
    ast_db: &AstDb,
    symbol_table: &mut SymbolTable,
) -> Option<HashMap<String, RegionProps>> {
    let mut bindings: HashMap<String, RegionProps> = HashMap::new();
    let mut ok = true;

    for (name, region) in &ast_db.regions {
        let mut binding = RegionProps {
            addr: 0,
            size: 0,
            name: name.clone(),
            default_pad_byte: 0xFF,
            src_loc: region.src_loc.clone(),
        };
        for prop_nid in ast.children(region.nid) {
            let tinfo = ast.get_tinfo(prop_nid);
            if tinfo.tok != LexToken::RegionProp {
                continue;
            }
            let prop_name = tinfo.val.to_string();
            let expr_nid = ast.children(prop_nid).next().unwrap();
            let expr_loc = ast.get_tinfo(expr_nid).loc.clone();

            match eval_expr_tree(ast, expr_nid, symbol_table, diags) {
                None => {
                    ok = false;
                }
                Some(val) => {
                    if !val.is_numeric() {
                        diags.err1(
                            "ERR_180",
                            &format!(
                                "Region property '{}' must evaluate to a numeric value.",
                                prop_name
                            ),
                            expr_loc,
                        );
                        ok = false;
                        continue;
                    }
                    match prop_name.as_str() {
                        "addr" => binding.addr = val.to_u64(),
                        "size" => binding.size = val.to_u64(),
                        "default_pad_byte" => {
                            let pad_val = val.to_u64();
                            if pad_val > 255 {
                                diags.err1(
                                    "ERR_181",
                                    "Region property 'default_pad_byte' must be in range 0 to 255.",
                                    expr_loc,
                                );
                                ok = false;
                                continue;
                            }
                            binding.default_pad_byte = pad_val as u8;
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
        bindings.insert(name.clone(), binding);
    }

    for (name, binding) in &bindings {
        if binding.size > 0 && binding.addr.checked_add(binding.size).is_none() {
            diags.err1(
                "ERR_188",
                &format!(
                    "Region '{}' addr {:#X} + size {:#X} overflows u64.",
                    name, binding.addr, binding.size
                ),
                binding.src_loc.clone(),
            );
            ok = false;
        }
    }
    if ok { Some(bindings) } else { None }
}

/// Resolves obj block property values against the symbol table.
/// Returns a map of obj name -> ObjProps, or None on error.
pub fn evaluate_obj_props(
    diags: &mut Diags,
    ast: &Ast,
    ast_db: &AstDb,
    symbol_table: &mut SymbolTable,
) -> Option<HashMap<String, ObjProps>> {
    let mut props: HashMap<String, ObjProps> = HashMap::new();
    let mut ok = true;

    for (obj_name, obj_decl) in &ast_db.obj_decls {
        let file    = resolve_obj_prop(obj_decl.nid, ast, "file",    obj_name, &obj_decl.src_loc, symbol_table, diags);
        let section = resolve_obj_prop(obj_decl.nid, ast, "section", obj_name, &obj_decl.src_loc, symbol_table, diags);
        // Optional properties: None signals resolution failure; Some(s) includes the empty string
        // for absent properties.
        let file_exclude    = resolve_optional_obj_prop(obj_decl.nid, ast, "file_exclude",    obj_name, &obj_decl.src_loc, symbol_table, diags);
        let section_exclude = resolve_optional_obj_prop(obj_decl.nid, ast, "section_exclude", obj_name, &obj_decl.src_loc, symbol_table, diags);
        match (file, section, file_exclude, section_exclude) {
            (Some(f), Some(s), Some(fe), Some(se)) => {
                props.insert(obj_name.clone(), ObjProps {
                    file: f,
                    name: s,
                    file_exclude: fe,
                    section_exclude: se,
                    src_loc: obj_decl.src_loc.clone(),
                });
            }
            _ => ok = false,
        }
    }
    if ok { Some(props) } else { None }
}

fn resolve_obj_prop(
    obj_nid: NodeId,
    ast: &Ast,
    prop: &str,
    obj_name: &str,
    src_loc: &diags::SourceSpan,
    symbol_table: &mut SymbolTable,
    diags: &mut Diags,
) -> Option<String> {
    let val_nid = ast
        .children(obj_nid)
        .filter_map(|prop_nid| {
            let tinfo = ast.get_tinfo(prop_nid);
            if tinfo.tok == LexToken::ObjProp && tinfo.val == prop {
                ast.children(prop_nid).next()
            } else {
                None
            }
        })
        .next()
        .unwrap(); // parser guarantees both section and file are present
    resolve_string_value(ast.get_tinfo(val_nid), obj_name, prop, src_loc, symbol_table, diags)
}

/// Resolves an optional obj property.  Returns `Some("")` if the property is
/// absent, `Some(s)` if present and resolved, `None` if present but resolution
/// failed (error already emitted).
fn resolve_optional_obj_prop(
    obj_nid: NodeId,
    ast: &Ast,
    prop: &str,
    obj_name: &str,
    src_loc: &diags::SourceSpan,
    symbol_table: &mut SymbolTable,
    diags: &mut Diags,
) -> Option<String> {
    let Some(val_nid) = ast
        .children(obj_nid)
        .filter_map(|prop_nid| {
            let tinfo = ast.get_tinfo(prop_nid);
            if tinfo.tok == LexToken::ObjProp && tinfo.val == prop {
                ast.children(prop_nid).next()
            } else {
                None
            }
        })
        .next()
    else {
        return Some(String::new()); // property absent -- no exclusion
    };
    resolve_string_value(ast.get_tinfo(val_nid), obj_name, prop, src_loc, symbol_table, diags)
}

fn resolve_string_value(
    val_tinfo: &ast::TokenInfo<'_>,
    obj_name: &str,
    prop: &str,
    src_loc: &diags::SourceSpan,
    symbol_table: &mut SymbolTable,
    diags: &mut Diags,
) -> Option<String> {
    match val_tinfo.tok {
        LexToken::QuotedString => Some(
            ir::unescape_string(
                val_tinfo.val
                    .strip_prefix('"')
                    .unwrap_or("")
                    .strip_suffix('"')
                    .unwrap_or("")
            ),
        ),
        LexToken::Identifier => {
            let name = val_tinfo.val;
            match symbol_table.get_value(name) {
                Some(ParameterValue::QuotedString(s)) => {
                    symbol_table.mark_used(name);
                    Some(s)
                }
                Some(_) => {
                    let m = format!(
                        "obj '{}': '{}' const '{}' must be a string value",
                        obj_name, prop, name
                    );
                    diags.err1("ERR_228", &m, src_loc.clone());
                    None
                }
                None => {
                    let m = format!(
                        "obj '{}': '{}' const '{}' is not defined",
                        obj_name, prop, name
                    );
                    diags.err1("ERR_229", &m, src_loc.clone());
                    None
                }
            }
        }
        _ => unreachable!("obj property value must be QuotedString or Identifier"),
    }
}

// ── Top-Level AST Walker & Pruner ─────────────────────────────────────────────

fn get_if_branches(ast: &Ast, if_nid: NodeId) -> (Vec<NodeId>, Vec<NodeId>) {
    let children: Vec<NodeId> = ast.children(if_nid).collect();
    let then_close_idx = children[2..]
        .iter()
        .position(|&n| ast.get_tinfo(n).tok == LexToken::CloseBrace)
        .map(|i| i + 2)
        .unwrap();
    let then_stmts = children[2..then_close_idx].to_vec();

    let else_stmts = if then_close_idx + 1 < children.len() {
        let after_else_idx = then_close_idx + 2;
        if after_else_idx >= children.len() {
            vec![]
        } else {
            let after_else_nid = children[after_else_idx];
            if ast.get_tinfo(after_else_nid).tok == LexToken::If {
                vec![after_else_nid]
            } else {
                let else_close_idx = children[after_else_idx + 1..]
                    .iter()
                    .position(|&n| ast.get_tinfo(n).tok == LexToken::CloseBrace)
                    .map(|i| i + after_else_idx + 1)
                    .unwrap();
                children[after_else_idx + 1..else_close_idx].to_vec()
            }
        }
    } else {
        vec![]
    };
    (then_stmts, else_stmts)
}

fn walk_if_statement(
    ast: &Ast,
    if_nid: NodeId,
    symbol_table: &mut SymbolTable,
    diags: &mut Diags,
) -> bool {
    let mut it = ast.children(if_nid);
    let cond_nid = it.next().unwrap();
    let cond_loc = ast.get_tinfo(cond_nid).loc.clone();

    let cond_val = eval_expr_tree(ast, cond_nid, symbol_table, diags);
    let b = match cond_val.and_then(|v| v.to_bool()) {
        Some(v) => v,
        None => {
            diags.err1(
                "ERR_112",
                "if condition must evaluate to a numeric type",
                cond_loc,
            );
            return false;
        }
    };

    let (then_stmts, else_stmts) = get_if_branches(ast, if_nid);
    let taken = if b { then_stmts } else { else_stmts };

    let mut ok = true;
    for stmt_nid in taken {
        ok &= evaluate_stmt(ast, stmt_nid, symbol_table, diags);
    }
    ok
}

fn evaluate_stmt(
    ast: &Ast,
    nid: NodeId,
    symbol_table: &mut SymbolTable,
    diags: &mut Diags,
) -> bool {
    let tinfo = ast.get_tinfo(nid);
    match tinfo.tok {
        LexToken::Const => {
            let mut it = ast.children(nid);
            let name_nid = it.next().unwrap();
            let name = ast.get_tinfo(name_nid).val.to_string();
            let second_nid = it.next().unwrap();
            if ast.get_tinfo(second_nid).tok == LexToken::Eq {
                let expr_nid = it.next().unwrap();
                if let Some(val) = eval_expr_tree(ast, expr_nid, symbol_table, diags) {
                    if !symbol_table.contains_key(&name) {
                        symbol_table.define(name, val, Some(tinfo.loc.clone()));
                    }
                    true
                } else {
                    false
                }
            } else {
                symbol_table.declare(name, tinfo.loc.clone());
                true
            }
        }
        LexToken::Eq => {
            let mut it = ast.children(nid);
            let name_nid = it.next().unwrap();
            let expr_nid = it.next().unwrap();
            let name = ast.get_tinfo(name_nid).val.to_string();
            if let Some(val) = eval_expr_tree(ast, expr_nid, symbol_table, diags) {
                symbol_table.assign(&name, val, &tinfo.loc, diags)
            } else {
                false
            }
        }
        LexToken::Print if !diags.noprint => {
            let mut s = String::new();
            let mut ok = true;
            for expr_nid in ast.children(nid) {
                match eval_expr_tree(ast, expr_nid, symbol_table, diags) {
                    Some(ParameterValue::QuotedString(v)) => s.push_str(&v),
                    Some(ParameterValue::U64(v)) => s.push_str(&format!("{:#X}", v)),
                    Some(ParameterValue::I64(v) | ParameterValue::Integer(v)) => {
                        s.push_str(&format!("{}", v))
                    }
                    Some(_) => {
                        diags.err1(
                            "ERR_97",
                            "Cannot print this value type in a const context",
                            tinfo.loc.clone(),
                        );
                        ok = false;
                    }
                    None => {
                        ok = false;
                    }
                }
            }
            if ok {
                print!("{}", s);
            }
            ok
        }
        LexToken::Assert => {
            let expr_nid = ast.children(nid).next().unwrap();
            match eval_expr_tree(ast, expr_nid, symbol_table, diags).and_then(|v| v.to_bool()) {
                Some(false) => {
                    diags.err1(
                        "ERR_98",
                        "Assert expression failed in if/else body",
                        tinfo.loc.clone(),
                    );
                    false
                }
                None => {
                    diags.err1(
                        "ERR_113",
                        "assert condition must evaluate to a numeric type",
                        tinfo.loc.clone(),
                    );
                    false
                }
                Some(true) => true,
            }
        }
        LexToken::If => walk_if_statement(ast, nid, symbol_table, diags),

        // All other statements are ignored by the const evaluator.
        // These include section, region, output and more.
        _ => true,
    }
}

// ── Pruning (AST Rewriter) ────────────────────────────────────────────────────

fn prune_body(
    pruned: &mut Ast,
    parent_nid: NodeId,
    symbol_table: &mut SymbolTable,
    diags: &mut Diags,
    keep: fn(LexToken) -> bool,
) -> anyhow::Result<()> {
    loop {
        let children: Vec<_> = pruned.children(parent_nid).collect();
        let maybe_if = children
            .iter()
            .find(|&&nid| pruned.get_tinfo(nid).tok == LexToken::If)
            .copied();
        match maybe_if {
            None => break,
            Some(if_nid) => prune_if_node(pruned, if_nid, symbol_table, diags, keep)?,
        }
    }
    Ok(())
}

fn prune_if_node(
    pruned: &mut Ast,
    if_nid: NodeId,
    symbol_table: &mut SymbolTable,
    diags: &mut Diags,
    keep: fn(LexToken) -> bool,
) -> anyhow::Result<()> {
    let cond_nid = pruned.children(if_nid).next().unwrap();
    let cond_val = eval_expr_tree(pruned, cond_nid, symbol_table, diags)
        .and_then(|v| v.to_bool())
        .unwrap_or(false);

    let (then_stmts, else_stmts) = get_if_branches(pruned, if_nid);
    let stmts_to_promote = if cond_val { &then_stmts } else { &else_stmts };

    for &stmt_nid in stmts_to_promote {
        if keep(pruned.get_tinfo(stmt_nid).tok) {
            stmt_nid.detach(pruned.arena_mut());
            if_nid.insert_before(stmt_nid, pruned.arena_mut());
        }
    }
    if_nid.detach(pruned.arena_mut());
    Ok(())
}

// ── Public Interface ──────────────────────────────────────────────────────────

pub fn evaluate_and_prune<'a>(
    diags: &mut Diags,
    ast: &Ast<'a>,
    ast_db: &AstDb,
    defines: &HashMap<String, ParameterValue>,
) -> anyhow::Result<(SymbolTable, Ast<'a>)> {
    debug!("const_eval::evaluate_and_prune: ENTER");
    let mut symbol_table = SymbolTable::new();
    for (k, v) in defines {
        symbol_table.define(k.clone(), v.clone(), None);
    }

    let mut ok = true;
    for &nid in &ast_db.const_statements {
        ok &= evaluate_stmt(ast, nid, &mut symbol_table, diags);
    }
    if !ok {
        bail!("const_eval lowering failed.");
    }

    let mut pruned = ast.clone();
    let root_nid = pruned.root();
    prune_body(&mut pruned, root_nid, &mut symbol_table, diags, |tok| {
        matches!(tok, LexToken::Section | LexToken::If)
    })?;

    let section_nids: Vec<_> = pruned
        .children(pruned.root())
        .filter(|&nid| pruned.get_tinfo(nid).tok == LexToken::Section)
        .collect();
    for sec_nid in section_nids {
        prune_body(&mut pruned, sec_nid, &mut symbol_table, diags, |_| true)?;
    }

    debug!("const_eval::evaluate_and_prune: EXIT");
    Ok((symbol_table, pruned))
}
