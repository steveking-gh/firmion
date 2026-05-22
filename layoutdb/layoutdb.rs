// AST to linear IR lowering for layout-time statements.
//
// LayoutDb walks the AST and flattens the tree into two parallel vectors: a
// sequence of LinIR instructions and a sequence of LinOperand operands. During
// this pass, Firmion resolves section nesting and expression structure into
// linear order. Values are still stored as raw strings at this point.  Type
// conversion and expression evaluation happens later.
//
// Expression lowering (atoms, operators, extension calls) calls into the
// shared `linearizer` crate.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::depth_guard::{DepthGuard, MAX_RECURSION_DEPTH};
use diags::Diags;
use diags::SourceSpan;
use indextree::NodeId;

#[allow(unused_imports)]
use tracing::{debug, trace};

use ast::{Ast, LexToken, is_reserved_identifier};
use ast::astdb::AstDb;
use ir::{IRKind, ObjProps};
use std::collections::{HashMap, HashSet};
use ir::symtable::SymbolTable;

use const_eval::linearizer::{LinIR, LinOperand, Linearizer, tok_to_irkind};

// ── LayoutDb ──────────────────────────────────────────────────────────────────

pub struct LayoutDb {
    /// Flat, ordered sequence of layout-time IR instructions.
    pub ir_vec: Vec<LinIR>,

    /// Operands referenced by ir_vec instructions.
    pub operand_vec: Vec<LinOperand>,

    pub output_sec_str: String,
    pub output_sec_loc: SourceSpan,

    /// Names of every section declared in the source (used by irdb).
    pub section_names: HashSet<String>,

    /// Names of every region declared in the source.
    /// Allows IdentDb to accept region names in addr() and sizeof() before
    /// layout_phase resolves them from irdb.region_bindings.
    pub region_names: HashSet<String>,

    /// Obj declarations from the source, unresolved; consumed by IRDb.
    pub obj_props: HashMap<String, ObjProps>,
}

impl LayoutDb {
    pub fn dump(&self) {
        for (idx, ir) in self.ir_vec.iter().enumerate() {
            let mut op = format!("lid {}: nid {} is {:?}", idx, ir.nid, ir.op);
            let mut first = true;
            for child in &ir.operand_vec {
                let operand = &self.operand_vec[*child];
                if !first {
                    op.push(',');
                } else {
                    first = false;
                }
                match operand {
                    LinOperand::Literal { sval, .. }
                    | LinOperand::Ref { sval, .. }
                    | LinOperand::NameDef { sval, .. } => op.push_str(&format!(" {}", sval)),
                    LinOperand::Output { ir_lid, .. } => {
                        op.push_str(&format!(" tmp{}, output of lid {}", *child, ir_lid))
                    }
                }
            }
            debug!("LayoutDb: {}", op);
        }
    }
}

// ── Static linearization helpers ─────────────────────────────────────────────
//
// All record_* functions are associated functions (no &mut self).  Each takes
// a &mut Linearizer for the relevant IR target (layout or const) plus any
// other context needed.  This eliminates the former in_const_expr: bool
// flag and the self-borrow tension that flag caused.

impl<'toks> LayoutDb {
    // ── Layout: section-body recursion ───────────────────────────────────────

    /// Recurse over each child of parent_nid calling record_r.
    #[allow(clippy::too_many_arguments)]
    fn record_children_r(
        lz: &mut Linearizer,
        parent_nid: NodeId,
        lops: &mut Vec<usize>,
        symbol_table: &SymbolTable,
        diags: &mut Diags,
        ast: &'toks Ast,
        ast_db: &AstDb,
        obj_props: &HashMap<String, ObjProps>,
    ) -> bool {
        let mut result = true;
        for nid in ast.children(parent_nid) {
            result &= Self::record_r(lz, nid, lops, symbol_table, diags, ast, ast_db, obj_props);
        }
        result
    }

    /// Lower one layout-time AST node.
    ///
    /// Expression tokens are delegated to `lz.record_expr_r()`.
    /// Layout statement tokens (section, wr*, align, etc.) are handled here.
    #[allow(clippy::too_many_arguments)]
    fn record_r(
        lz: &mut Linearizer,
        parent_nid: NodeId,
        returned_operands: &mut Vec<usize>,
        symbol_table: &SymbolTable,
        diags: &mut Diags,
        ast: &'toks Ast,
        ast_db: &AstDb,
        obj_props: &HashMap<String, ObjProps>,
    ) -> bool {
        debug!("LayoutDb::record_r: ENTER for parent nid: {}", parent_nid);

        let Some(_guard) = DepthGuard::enter(MAX_RECURSION_DEPTH) else {
            let tinfo = ast.get_tinfo(parent_nid);
            let m = format!(
                "Maximum recursion depth ({MAX_RECURSION_DEPTH}) exceeded when processing '{}'.",
                tinfo.val
            );
            diags.err1("ERR_215", &m, tinfo.span());
            return false;
        };

        let tinfo = ast.get_tinfo(parent_nid);
        let tok = tinfo.tok;
        let mut result = true;

        match tok {
            // ── Const reference operand ───────────────────────────────────
            // Handles the case where `const` appears as a child operand.
            LexToken::Const => {
                let idx = lz.operand_vec.len();
                lz.operand_vec
                    .push(LinOperand::new_literal(tinfo, ir::DataType::Unknown));
                returned_operands.push(idx);
            }

            // ── Generic wr: obj write, section write, or extension write ─────
            LexToken::Wr => {
                let mut lops = Vec::new();
                let child_nid = ast.children(parent_nid).next().unwrap();
                let child_tinfo = ast.get_tinfo(child_nid);

                if child_tinfo.tok == LexToken::Identifier && !ast.has_children(child_nid) {
                    let sec_name_str = child_tinfo.val;
                    if obj_props.contains_key(sec_name_str) {
                        // wr obj_name — emit Wrobj with single Name operand.
                        let ir_lid = lz.new_ir(parent_nid, ast, IRKind::Wrobj);
                        lz.add_new_operand_to_ir(ir_lid, LinOperand::new_name(child_tinfo));
                    } else {
                        let section = ast_db.sections.get(sec_name_str).unwrap();
                        let sec_nid = section.nid;
                        result &= Self::record_r(
                            lz,
                            sec_nid,
                            &mut lops,
                            symbol_table,
                            diags,
                            ast,
                            ast_db,
                            obj_props,
                        );
                        result &= lz.operand_count_is_valid(0, &lops, diags, tinfo);
                    }
                } else {
                    // record_expr_r (called via record_children_r) creates the
                    // extension call LinIR as the write statement directly.
                    // No WrExt wrapper is needed.
                    result &= Self::record_children_r(
                        lz,
                        parent_nid,
                        &mut lops,
                        symbol_table,
                        diags,
                        ast,
                        ast_db,
                        obj_props,
                    );
                }
            }

            // ── sizeof(section) or sizeof(namespace::ext) ─────────────────
            LexToken::Sizeof => {
                let first_child = ast.children(parent_nid).next().unwrap();
                let first_child_tinfo = ast.get_tinfo(first_child);

                if first_child_tinfo.tok == LexToken::Namespace {
                    let ns_children: Vec<_> = ast.children(first_child).collect();
                    let ext_id_tinfo = ast.get_tinfo(ns_children[0]);
                    let full_name = format!("{}{}", first_child_tinfo.val, ext_id_tinfo.val);

                    let ir_lid = lz.new_ir(parent_nid, ast, IRKind::SizeofExt);
                    // Store the full qualified name (e.g. "custom::foo") in sval.
                    // tok is Identifier to match the simple sizeof(section) case below.
                    lz.add_new_operand_to_ir(
                        ir_lid,
                        LinOperand::new_name_str(full_name, first_child_tinfo.loc.clone()),
                    );

                    let idx = lz.add_new_operand_to_ir(
                        ir_lid,
                        LinOperand::new_output(ir_lid, tinfo.loc.clone(), ir::DataType::U64),
                    );
                    returned_operands.push(idx);
                } else {
                    let mut lops = Vec::new();
                    let ir_lid = lz.new_ir(parent_nid, ast, IRKind::Sizeof);
                    result &=
                        lz.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                    result &= lz.process_operands(1, &mut lops, ir_lid, diags, tinfo);
                    let idx = lz.add_new_operand_to_ir(
                        ir_lid,
                        LinOperand::new_output(ir_lid, tinfo.loc.clone(), ir::DataType::U64),
                    );
                    returned_operands.push(idx);
                }
            }

            // ── Address queries ───────────────────────────────────────────
            LexToken::Addr | LexToken::AddrOffset | LexToken::SecOffset | LexToken::FileOffset => {
                let mut lops = Vec::new();
                let ir_lid = lz.new_ir(parent_nid, ast, tok_to_irkind(tok));
                result &=
                    lz.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                result &= lz.process_optional_operands(1, &mut lops, ir_lid, diags, tinfo);
                let idx = lz.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), ir::DataType::U64),
                );
                returned_operands.push(idx);
            }

            // ── Alignment and address directives ──────────────────────────
            LexToken::PadSecOffset
            | LexToken::PadAddrOffset
            | LexToken::SetAddr
            | LexToken::PadFileOffset
            | LexToken::Align => {
                let mut lops = Vec::new();
                let ir_lid = lz.new_ir(parent_nid, ast, tok_to_irkind(tok));
                result &=
                    lz.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);

                if lops.len() != 1 && lops.len() != 2 {
                    let m = format!(
                        "{:?} requires 1 or 2 operands, but found {}",
                        tok,
                        lops.len()
                    );
                    diags.err1("ERR_210", &m, tinfo.span());
                    return false;
                }

                lz.add_existing_operand_to_ir(ir_lid, lops[0]);
                let count_output = lz.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_output(ir_lid, tinfo.loc.clone(), ir::DataType::U64),
                );

                let wr8_lid = lz.new_ir(parent_nid, ast, IRKind::Wr(1, false));

                if lops.len() == 2 {
                    lz.add_existing_operand_to_ir(wr8_lid, lops[1]);
                } else {
                    // Synthesize a literal 0 pad byte — no source token exists for this value.
                    lz.add_new_operand_to_ir(
                        wr8_lid,
                        LinOperand::Literal {
                            src_loc: tinfo.loc.clone(),
                            tok: LexToken::Integer,
                            sval: "0".to_string(),
                            param_name: None,
                            data_type: ir::DataType::Integer,
                        },
                    );
                }
                lz.add_existing_operand_to_ir(wr8_lid, count_output);
            }

            // ── Write and print statements ────────────────────────────────
            LexToken::Assert
            | LexToken::Wr8
            | LexToken::Wr16
            | LexToken::Wr24
            | LexToken::Wr32
            | LexToken::Wr40
            | LexToken::Wr48
            | LexToken::Wr56
            | LexToken::Wr64
            | LexToken::Wrbe8
            | LexToken::Wrbe16
            | LexToken::Wrbe24
            | LexToken::Wrbe32
            | LexToken::Wrbe40
            | LexToken::Wrbe48
            | LexToken::Wrbe56
            | LexToken::Wrbe64
            | LexToken::Wrs
            | LexToken::Wrf
            | LexToken::Print
            | LexToken::Trace => {
                let mut lops = Vec::new();
                result &=
                    lz.record_expr_children_r(parent_nid, &mut lops, symbol_table, diags, ast);
                let ir_lid = lz.new_ir(parent_nid, ast, tok_to_irkind(tok));
                for idx in lops {
                    lz.add_existing_operand_to_ir(ir_lid, idx);
                }
            }

            // ── Section block ─────────────────────────────────────────────
            LexToken::Section => {
                // First child is the section name; emit as Name (structural,
                // the linearizer never const-substitutes the name) then process the rest as body.
                let mut children = ast.children(parent_nid);
                let name_tinfo = ast.get_tinfo(children.next().unwrap());
                let name_idx = lz.operand_vec.len();
                lz.operand_vec.push(LinOperand::new_name(name_tinfo));

                let start_lid = lz.new_ir(parent_nid, ast, IRKind::SectionStart);
                lz.add_existing_operand_to_ir(start_lid, name_idx);

                let mut dummy = Vec::new();
                for child_nid in children {
                    result &= Self::record_r(
                        lz,
                        child_nid,
                        &mut dummy,
                        symbol_table,
                        diags,
                        ast,
                        ast_db,
                        obj_props,
                    );
                }

                let end_lid = lz.new_ir(parent_nid, ast, IRKind::SectionEnd);
                lz.add_existing_operand_to_ir(end_lid, name_idx);
            }

            // ── Label declaration ─────────────────────────────────────────
            LexToken::Label => {
                let ir_lid = lz.new_ir(parent_nid, ast, IRKind::Label);
                // Strip the trailing ':' from the label token text.
                let name_without_colon = tinfo.val[..tinfo.val.len() - 1].to_string();
                lz.add_new_operand_to_ir(
                    ir_lid,
                    LinOperand::new_name_str(name_without_colon, tinfo.loc.clone()),
                );
            }

            // ── Error arms ────────────────────────────────────────────────
            LexToken::Unknown => {
                diags.err1("ERR_216", "Unexpected character.", tinfo.span());
                result = false;
            }
            LexToken::Output => {
                let m = format!("Unexpected '{}' expression not allowed here.", tinfo.val);
                diags.err1("ERR_217", &m, tinfo.span());
                result = false;
            }
            LexToken::If | LexToken::Else => {
                let m = format!("Unexpected '{}' in linearization context.", tinfo.val);
                diags.err1("ERR_213", &m, tinfo.span());
                result = false;
            }

            // ── All expression tokens: delegate to the shared linearizer ──
            _ => {
                result = lz.record_expr_r(parent_nid, returned_operands, symbol_table, diags, ast);
            }
        }

        debug!(
            "LayoutDb::record_r: EXIT({}) for nid: {}",
            result, parent_nid
        );
        result
    }

    // ── Constructor ───────────────────────────────────────────────────────────

    pub fn new(
        diags: &mut Diags,
        ast: &'toks Ast,
        ast_db: &AstDb,
        symbol_table: &SymbolTable,
        obj_props: HashMap<String, ObjProps>,
    ) -> anyhow::Result<Self> {
        debug!("LayoutDb::new: ENTER");

        let output_nid = ast_db.output.nid;
        let output_sec_tinfo = ast.get_tinfo(ast_db.output.sec_nid);
        let output_sec_str = output_sec_tinfo.val.to_string();
        let output_sec_loc = output_sec_tinfo.loc.clone();
        debug!("LayoutDb::new: Output section name is {}", output_sec_str);

        let section_names: HashSet<String> =
            ast_db.sections.keys().map(|s| s.to_string()).collect();
        let region_names: HashSet<String> = ast_db.regions.keys().map(|s| s.to_string()).collect();
        let mut layout_lz = Linearizer::new();

        // Linearize pre-output top-level statements first.
        for &nid in ast_db.pre_output_globals.iter() {
            let mut lops = Vec::new();
            if !Self::record_r(
                &mut layout_lz,
                nid,
                &mut lops,
                symbol_table,
                diags,
                ast,
                ast_db,
                &obj_props,
            ) {
                anyhow::bail!("LayoutDb construction failed.");
            }
        }

        // Linearize the output section body (layout-time IR).
        let section = ast_db.sections.get(output_sec_str.as_str()).unwrap();
        let sec_nid = section.nid;
        let mut lops = Vec::new();
        if !Self::record_r(
            &mut layout_lz,
            sec_nid,
            &mut lops,
            symbol_table,
            diags,
            ast,
            ast_db,
            &obj_props,
        ) {
            anyhow::bail!("LayoutDb construction failed.");
        }

        // Sentinel: everything after this point executes after the output file is written.
        layout_lz.new_ir(output_nid, ast, IRKind::Output);

        // Linearize post-output top-level statements last.
        for &nid in ast_db.post_output_globals.iter() {
            let mut lops = Vec::new();
            if !Self::record_r(
                &mut layout_lz,
                nid,
                &mut lops,
                symbol_table,
                diags,
                ast,
                ast_db,
                &obj_props,
            ) {
                anyhow::bail!("LayoutDb construction failed.");
            }
        }

        // Extract the vectors from the Linearizer instances into LayoutDb.
        let layout_db = LayoutDb {
            ir_vec: layout_lz.ir_vec,
            operand_vec: layout_lz.operand_vec,
            output_sec_str,
            output_sec_loc,
            section_names,
            region_names,
            obj_props,
        };

        layout_db.dump();

        if !IdentDb::check_globals(&layout_db, diags) {
            anyhow::bail!("LayoutDb construction failed.");
        }
        if !IdentDb::check_locals(&layout_db, diags) {
            anyhow::bail!("LayoutDb construction failed.");
        }

        debug!("LayoutDb::new: EXIT for nid: {}", output_nid);
        Ok(layout_db)
    }
}

// ── IdentDb ───────────────────────────────────────────────────────────────────

struct IdentDb {
    label_idents: HashMap<String, SourceSpan>,
    section_count: HashMap<String, usize>,
}

impl IdentDb {
    pub fn new() -> IdentDb {
        IdentDb {
            label_idents: HashMap::new(),
            section_count: HashMap::new(),
        }
    }

    pub fn check_globals(lindb: &LayoutDb, diags: &mut Diags) -> bool {
        let mut idb = IdentDb::new();
        if !idb.inventory_global_identifiers(lindb, diags) {
            return false;
        }
        if !idb.verify_global_refs(lindb, diags) {
            return false;
        }
        true
    }

    pub fn check_locals(lindb: &LayoutDb, diags: &mut Diags) -> bool {
        debug!("IdentDb::check_locals: ENTER");
        let mut result = true;
        let mut lid = 0;
        let len = lindb.ir_vec.len();
        while lid < len && lindb.ir_vec[lid].op != IRKind::SectionStart {
            lid += 1;
        }
        lid += 1;
        result &= IdentDb::check_locals_r(&mut lid, lindb, diags);
        debug!("IdentDb::check_locals: EXIT({})", result);
        result
    }

    fn check_locals_r(lid: &mut usize, lindb: &LayoutDb, diags: &mut Diags) -> bool {
        debug!("IdentDb::check_locals_r: ENTER at lid {}", *lid);
        let mut result = true;
        let mut idb = IdentDb::new();
        let start_lid = *lid;
        loop {
            let lir = &lindb.ir_vec[*lid];
            *lid += 1;
            match lir.op {
                IRKind::SectionStart => {
                    idb.inventory_section_identifiers(lir, lindb);
                    result &= IdentDb::check_locals_r(lid, lindb, diags);
                }
                IRKind::Label => {
                    idb.inventory_label_identifiers(0, lir, lindb, diags);
                }
                IRKind::SectionEnd => break,
                _ => {}
            }
        }
        if result {
            result &= idb.verify_local_refs(start_lid, lindb, diags);
        }
        debug!("IdentDb::check_locals_r: EXIT at lid {}", *lid);
        result
    }

    fn skip_nested_sections_r(start_lid: usize, lindb: &LayoutDb) -> usize {
        let mut lid = start_lid;
        loop {
            let lir = &lindb.ir_vec[lid];
            lid += 1;
            match lir.op {
                IRKind::SectionStart => {
                    lid = Self::skip_nested_sections_r(lid, lindb);
                }
                IRKind::SectionEnd => break,
                _ => {}
            }
        }
        lid
    }

    fn verify_local_refs(&self, start_lid: usize, lindb: &LayoutDb, diags: &mut Diags) -> bool {
        let mut result = true;
        let mut lid = start_lid;
        loop {
            let lir = &lindb.ir_vec[lid];
            lid += 1;
            match lir.op {
                IRKind::SecOffset => {
                    result &= self.verify_operand_refs(lir, lindb, diags);
                }
                IRKind::SectionStart => {
                    lid = Self::skip_nested_sections_r(lid, lindb);
                }
                IRKind::SectionEnd => break,
                _ => {}
            }
        }
        result
    }

    fn inventory_label_identifiers(
        &mut self,
        op_num: usize,
        lir: &LinIR,
        lindb: &LayoutDb,
        diags: &mut Diags,
    ) -> bool {
        let mut result = true;
        let name_operand_num = lir.operand_vec[op_num];
        let name_operand = lindb.operand_vec.get(name_operand_num).unwrap();
        let LinOperand::NameDef {
            sval: name,
            src_loc,
        } = name_operand
        else {
            panic!("label identifier operand must be a NameDef operand type!");
        };
        if is_reserved_identifier(name) {
            let m = format!(
                "'{}' is a reserved identifier and cannot be used as a label name",
                name
            );
            diags.err1("ERR_212", &m, src_loc.clone());
            return false;
        }
        if self.label_idents.contains_key(name) {
            let orig_loc = self.label_idents.get(name).unwrap();
            let msg = format!("Duplicate label name {}", name);
            diags.err2("ERR_204", &msg, src_loc.clone(), orig_loc.clone());
            result = false;
        } else {
            self.label_idents.insert(name.clone(), src_loc.clone());
        }
        result
    }

    fn inventory_section_identifiers(&mut self, lir: &LinIR, lindb: &LayoutDb) {
        trace!("IdentDb::inventory_section_identifiers: ENTER");
        let name_operand_num = lir.operand_vec[0];
        let name_operand = lindb.operand_vec.get(name_operand_num).unwrap();
        let LinOperand::NameDef { sval: name, .. } = name_operand else {
            panic!("section identifier operand must be a NameDef operand type!");
        };
        debug!(
            "IdentDb::inventory_section_identifiers: Adding section name {} to inventory.",
            name
        );
        // Increment the count for this section name or create a new entry.
        *self.section_count.entry(name.to_string()).or_insert(0) += 1;
        trace!("IdentDb::inventory_section_identifiers: EXIT");
    }

    fn inventory_global_identifiers(&mut self, lindb: &LayoutDb, diags: &mut Diags) -> bool {
        let mut result = true;
        for lir in &lindb.ir_vec {
            result &= match lir.op {
                IRKind::Label => self.inventory_label_identifiers(0, lir, lindb, diags),
                IRKind::SectionStart => {
                    self.inventory_section_identifiers(lir, lindb);
                    true
                }
                _ => true,
            }
        }
        debug!("IdentDb::inventory_global_identifiers:");
        for name in self.label_idents.keys() {
            debug!("    {}", name);
        }
        result
    }

    fn verify_global_refs(&self, lindb: &LayoutDb, diags: &mut Diags) -> bool {
        let mut result = true;
        for lir in &lindb.ir_vec {
            result &= match lir.op {
                IRKind::Addr | IRKind::AddrOffset | IRKind::FileOffset | IRKind::Sizeof => {
                    self.verify_operand_refs(lir, lindb, diags)
                }
                _ => true,
            }
        }
        result
    }

    fn is_valid_section_ref(&self, lop: &LinOperand, diags: &mut Diags) -> bool {
        let LinOperand::Ref { sval, src_loc, .. } = lop else {
            return false;
        };
        if let Some(count) = self.section_count.get(sval) {
            if *count == 1 {
                return true;
            }
            let msg = format!(
                "Reference to section '{}' is ambiguous. This section occurs {} times in the output",
                sval, *count
            );
            diags.err1("ERR_209", &msg, src_loc.clone());
        }
        false
    }

    fn is_valid_label_ref(&self, lop: &LinOperand) -> bool {
        let LinOperand::Ref { sval, .. } = lop else {
            return false;
        };
        self.label_idents.contains_key(sval)
    }

    fn verify_operand_refs(&self, lir: &LinIR, lindb: &LayoutDb, diags: &mut Diags) -> bool {
        let mut result = true;
        for &lop_num in &lir.operand_vec {
            let lop = &lindb.operand_vec[lop_num];
            // Only Ref operands (identifier value references) need ref checks.
            let LinOperand::Ref { sval, src_loc, .. } = lop else {
                continue;
            };
            {
                debug!(
                    "IdentDb::verify_identifier_refs: Verifying reference to '{}'",
                    sval
                );
                if self.is_valid_section_ref(lop, diags) {
                    continue;
                }
                if self.is_valid_label_ref(lop) {
                    if lir.op == IRKind::Sizeof {
                        let msg = "Sizeof cannot refer to a label name.  Labels have no size."
                            .to_string();
                        diags.err1("ERR_211", &msg, src_loc.clone());
                        result = false;
                    }
                    continue;
                }
                if lindb.region_names.contains(sval) {
                    continue;
                }
                if lindb.obj_props.contains_key(sval) {
                    continue;
                }
                let msg = format!("Unknown or unreachable identifier {}", sval);
                diags.err1("ERR_208", &msg, src_loc.clone());
                result = false;
            }
        }
        result
    }
}
