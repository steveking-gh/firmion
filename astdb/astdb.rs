// Indexed view of a parsed firmion AST.
//
// AstDb::new walks the top-level nodes of an Ast and builds named lookup maps
// for sections, regions, consts, and the output statement.  When validate is
// true, AstDb::new also walks the full nesting tree to catch cyclic wr chains
// and unknown wr targets.
//
// AstDb owns all of its data and holds no references into the Ast.  The Ast
// is only needed at construction time.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use anyhow::bail;
use ast::{Ast, LexToken, is_reserved_identifier};
use diags::depth_guard::{DepthGuard, MAX_RECURSION_DEPTH};
use diags::{Diags, SourceSpan};
use indextree::NodeId;
use std::collections::{HashMap, HashSet};

#[allow(unused_imports)]
use tracing::debug;

// -- Section -----------------------------------------------------------------

pub struct Section {
    /// Source location of the section keyword, for diagnostics.
    pub src_loc: SourceSpan,
    /// AST Node, which is an index into the arena storage of the Ast.
    pub nid: NodeId,
    /// Region name from an `in REGION` binding, if present.
    pub region: Option<String>,
}

impl Section {
    pub fn new(ast: &Ast, nid: NodeId) -> Section {
        // Second child is RegionRef when `section NAME in REGION` was parsed.
        let region = {
            let mut children = ast.children(nid);
            let _name = children.next(); // skip section name (first child)
            children.next().and_then(|child_nid| {
                let ti = ast.get_tinfo(child_nid);
                if ti.tok == LexToken::RegionRef {
                    Some(ti.val.to_string())
                } else {
                    None
                }
            })
        };
        Section {
            src_loc: ast.get_tinfo(nid).span(),
            nid,
            region,
        }
    }
}

// -- Label ---------------------------------------------------------------------

#[derive(Debug)]
pub struct Label {
    pub nid: NodeId,
    /// Source location of the label, for diagnostics.
    pub loc: SourceSpan,
}

// -- Output --------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Output {
    /// Source location of the output keyword, for diagnostics.
    pub src_loc: SourceSpan,
    pub nid: NodeId,
    pub sec_nid: NodeId,
}

impl Output {
    pub fn new(ast: &Ast, nid: NodeId) -> Output {
        let mut children = ast.children(nid);
        // The section name is the first child of the output.
        // AST processing guarantees this exists.
        let sec_nid = children.next().unwrap();
        Output {
            src_loc: ast.get_tinfo(nid).span(),
            nid,
            sec_nid,
        }
    }
}

// -- ObjDecl ------------------------------------------------------------------

/// AST-phase record of an `obj` declaration.
/// Holds the parse-tree position and source location; property values
/// (file, section) are extracted by the consumer via the AST node.
#[derive(Clone, Debug)]
pub struct ObjDecl {
    /// AST node ID of the obj root; used by irdb to find properties.
    pub nid: NodeId,
    /// Source location of the obj name, for diagnostics.
    pub src_loc: SourceSpan,
}

// -- Region --------------------------------------------------------------------

/// AST-phase record of a region declaration.
/// Holds the parse-tree position and source location; evaluated properties
/// (addr, size) are produced by const_eval::evaluate_regions.
#[derive(Clone, Debug)]
pub struct Region {
    /// AST node ID of the region root; used by const_eval to find properties.
    pub nid: NodeId,
    /// Source location of the region keyword, for diagnostics.
    pub src_loc: SourceSpan,
}

impl Region {
    pub fn new(nid: NodeId, src_loc: SourceSpan) -> Self {
        Region { nid, src_loc }
    }
}

// -- AstDb ---------------------------------------------------------------------

pub struct AstDb {
    pub sections: HashMap<String, Section>,
    pub labels: HashMap<String, Label>,
    pub output: Output,
    /// Top-level print and assert statements that appear before the `output` declaration.
    pub pre_output_globals: Vec<NodeId>,
    /// Top-level print statements that appear after the `output` declaration.
    /// assert is not permitted after output and is rejected during construction.
    pub post_output_globals: Vec<NodeId>,
    /// All top-level const definitions, declarations, and if/else blocks
    /// in their original source token order.
    pub const_statements: Vec<NodeId>,
    /// Set of all const names for collision detection.
    pub const_names: HashMap<String, SourceSpan>,
    /// All region declarations, keyed by name, in encounter order.
    pub regions: HashMap<String, Region>,
    /// All obj declarations, keyed by declared name, in encounter order.
    pub obj_decls: HashMap<String, ObjDecl>,
}

impl AstDb {
    fn record_obj(
        diags: &mut Diags,
        obj_nid: NodeId,
        ast: &Ast,
        obj_decls: &mut HashMap<String, ObjDecl>,
    ) -> bool {
        let mut children = ast.children(obj_nid);
        let name_nid = children.next().unwrap();
        let name_tinfo = ast.get_tinfo(name_nid);
        let name_str = name_tinfo.val;

        if is_reserved_identifier(name_str) {
            let m = format!("'{}' is a reserved identifier and cannot be used as an obj name", name_str);
            diags.err1("ERR_62", &m, name_tinfo.span());
            return false;
        }
        if let Some(existing) = obj_decls.get(name_str) {
            let m = format!("Duplicate obj name '{}'", name_str);
            diags.err2("ERR_63", &m, name_tinfo.span(), existing.src_loc.clone());
            return false;
        }

        obj_decls.insert(name_str.to_string(), ObjDecl {
            nid: obj_nid,
            src_loc: name_tinfo.span(),
        });
        true
    }

    fn record_region(
        diags: &mut Diags,
        reg_nid: NodeId,
        ast: &Ast,
        regions: &mut HashMap<String, Region>,
    ) -> bool {
        debug!("AstDb::record_region: NodeId {}", reg_nid);

        let mut children = ast.children(reg_nid);
        let name_nid = children.next().unwrap();
        let name_tinfo = ast.get_tinfo(name_nid);
        let name_str = name_tinfo.val;

        if is_reserved_identifier(name_str) {
            let m = format!(
                "'{}' is a reserved identifier and cannot be used as a region name",
                name_str
            );
            diags.err1("ERR_56", &m, name_tinfo.span());
            return false;
        }
        if let Some(existing) = regions.get(name_str) {
            let m = format!("Duplicate region name '{}'", name_str);
            diags.err2("ERR_55", &m, name_tinfo.span(), existing.src_loc.clone());
            return false;
        }

        let entry = Region::new(reg_nid, name_tinfo.span());
        regions.insert(name_str.to_string(), entry);
        true
    }

    fn record_section(
        diags: &mut Diags,
        sec_nid: NodeId,
        ast: &Ast,
        sections: &mut HashMap<String, Section>,
    ) -> bool {
        debug!("AstDb::record_section: NodeId {}", sec_nid);

        let mut children = ast.children(sec_nid);
        let sec_name_nid = children.next().unwrap();
        let sec_tinfo = ast.get_tinfo(sec_name_nid);
        let sec_str = sec_tinfo.val;
        if is_reserved_identifier(sec_str) {
            let m = format!(
                "'{}' is a reserved identifier and cannot be used as a section name",
                sec_str
            );
            diags.err1("ERR_27", &m, sec_tinfo.span());
            return false;
        }
        if let Some(orig_section) = sections.get(sec_str) {
            let m = format!("Duplicate section name '{}'", sec_str);
            diags.err2("ERR_24", &m, sec_tinfo.span(), orig_section.src_loc.clone());
            return false;
        }
        sections.insert(sec_str.to_string(), Section::new(ast, sec_nid));
        true
    }

    fn record_const(
        diags: &mut Diags,
        const_nid: NodeId,
        ast: &Ast,
        consts: &mut HashMap<String, SourceSpan>,
    ) -> bool {
        debug!("AstDb::record_const: NodeId {}", const_nid);

        let mut children = ast.children(const_nid);
        let const_name_nid = children.next().unwrap();
        let const_tinfo = ast.get_tinfo(const_name_nid);
        let const_str = const_tinfo.val;
        if is_reserved_identifier(const_str) {
            let m = format!(
                "'{}' is a reserved identifier and cannot be used as a const name",
                const_str
            );
            diags.err1("ERR_28", &m, const_tinfo.span());
            return false;
        }
        if let Some(orig_span) = consts.get(const_str) {
            let m = format!("Duplicate const name '{}'", const_str);
            diags.err2("ERR_25", &m, const_tinfo.span(), orig_span.clone());
            return false;
        }
        consts.insert(const_str.to_string(), const_tinfo.span());
        true
    }

    fn validate_section_name(
        &self,
        child_num: usize,
        parent_nid: NodeId,
        ast: &Ast,
        diags: &mut Diags,
    ) -> bool {
        debug!(
            "AstDb::validate_section_name: NodeId {} for child {}",
            parent_nid, child_num
        );

        let mut children = ast.children(parent_nid);

        let mut num = 0;
        while num < child_num {
            let sec_name_nid_opt = children.next();
            if sec_name_nid_opt.is_none() {
                let m = "Missing section name".to_string();
                let section_tinfo = ast.get_tinfo(parent_nid);
                diags.err1("ERR_20", &m, section_tinfo.span());
                return false;
            }
            num += 1;
        }
        let Some(sec_name_nid) = children.next() else {
            let m = "Missing section name".to_string();
            let section_tinfo = ast.get_tinfo(parent_nid);
            diags.err1("ERR_10", &m, section_tinfo.span());
            return false;
        };
        let sec_tinfo = ast.get_tinfo(sec_name_nid);
        let sec_str = sec_tinfo.val;
        if !self.sections.contains_key(sec_str) {
            let m = format!("Unknown or unreachable section name '{}'", sec_str);
            diags.err1("ERR_14", &m, sec_tinfo.span());
            return false;
        }
        true
    }

    pub fn record_output(
        diags: &mut Diags,
        nid: NodeId,
        ast: &Ast,
        output: &mut Option<Output>,
    ) -> bool {
        let tinfo = ast.get_tinfo(nid);
        if output.is_some() {
            let m = "Multiple output statements are not allowed.";
            let orig_src_loc = output.as_ref().unwrap().src_loc.clone();
            diags.err2("ERR_9", m, orig_src_loc, tinfo.span());
            return false;
        }
        *output = Some(Output::new(ast, nid));
        true
    }

    fn validate_nesting_r(
        &mut self,
        parent_nid: NodeId,
        ast: &Ast,
        nested_sections: &mut HashSet<String>,
        diags: &mut Diags,
    ) -> bool {
        debug!(
            "AstDb::validate_nesting_r: ENTER for parent nid: {}",
            parent_nid
        );

        let Some(_guard) = DepthGuard::enter(MAX_RECURSION_DEPTH) else {
            let tinfo = ast.get_tinfo(parent_nid);
            let m = format!(
                "Maximum recursion depth ({MAX_RECURSION_DEPTH}) exceeded when processing '{}'.",
                tinfo.val
            );
            diags.err1("ERR_4", &m, tinfo.span());
            return false;
        };

        let mut result = true;
        let tinfo = ast.get_tinfo(parent_nid);
        result &= match tinfo.tok {
            LexToken::Wr => {
                let mut children = ast.children(parent_nid);
                let sec_nid = children.next().unwrap();
                let sec_tinfo = ast.get_tinfo(sec_nid);

                if sec_tinfo.tok == LexToken::Identifier && !ast.has_children(sec_nid) {
                    let sec_str = sec_tinfo.val;

                    // `wr obj_name` — obj is a leaf, no section recursion needed.
                    if self.obj_decls.contains_key(sec_str) {
                        return true;
                    }

                    if !self.validate_section_name(0, parent_nid, ast, diags) {
                        return false;
                    }

                    if nested_sections.contains(sec_str) {
                        let m = "Writing section creates a cycle.";
                        diags.err1("ERR_5", m, sec_tinfo.span());
                        false
                    } else {
                        nested_sections.insert(sec_str.to_string());
                        let sec_nid = self.sections.get(sec_str).unwrap().nid;
                        let children = ast.children(sec_nid);
                        for nid in children {
                            result &= self.validate_nesting_r(nid, ast, nested_sections, diags);
                        }
                        nested_sections.remove(sec_str);
                        result
                    }
                } else {
                    true
                }
            }
            _ => {
                let children = ast.children(parent_nid);
                for nid in children {
                    result &= self.validate_nesting_r(nid, ast, nested_sections, diags);
                }
                result
            }
        };

        debug!(
            "AstDb::validate_nesting_r: EXIT({}) for nid: {}",
            result, parent_nid
        );
        result
    }

    /// Build an AstDb from ast.
    ///
    /// When validate is true (the normal post-prune call), the output section's
    /// full nesting tree is walked to catch circular references and unknown wr
    /// targets.  When false (the pre-prune call for const_eval), that walk is
    /// skipped so that wr references to sections inside top-level if blocks do
    /// not produce false-positive errors before pruning.
    pub fn new(diags: &mut Diags, ast: &Ast, validate: bool) -> anyhow::Result<AstDb> {
        debug!("AstDb::new");

        let mut result = true;
        let mut sections: HashMap<String, Section> = HashMap::new();
        let mut output: Option<Output> = None;
        let mut pre_output_globals: Vec<NodeId> = Vec::new();
        let mut post_output_globals: Vec<NodeId> = Vec::new();
        let mut output_seen = false;
        let mut const_statements: Vec<NodeId> = Vec::new();
        let mut const_names: HashMap<String, SourceSpan> = HashMap::new();
        let mut regions: HashMap<String, Region> = HashMap::new();
        let mut obj_decls: HashMap<String, ObjDecl> = HashMap::new();

        for nid in ast.children(ast.root()) {
            let tinfo = ast.get_tinfo(nid);
            result = result
                && match tinfo.tok {
                    LexToken::Section => Self::record_section(diags, nid, ast, &mut sections),
                    LexToken::Region => Self::record_region(diags, nid, ast, &mut regions),
                    LexToken::Obj => Self::record_obj(diags, nid, ast, &mut obj_decls),
                    LexToken::Output => {
                        output_seen = true;
                        Self::record_output(diags, nid, ast, &mut output)
                    }
                    LexToken::Const => {
                        const_statements.push(nid);
                        Self::record_const(diags, nid, ast, &mut const_names)
                    }
                    LexToken::If => {
                        const_statements.push(nid);
                        true
                    }
                    LexToken::Eq => {
                        const_statements.push(nid);
                        true
                    }
                    LexToken::Assert => {
                        if output_seen {
                            post_output_globals.push(nid);
                        } else {
                            pre_output_globals.push(nid);
                        }
                        true
                    }
                    LexToken::Print | LexToken::Trace => {
                        if output_seen {
                            post_output_globals.push(nid);
                        } else {
                            pre_output_globals.push(nid);
                        }
                        true
                    }
                    _ => {
                        let msg = format!("Invalid top-level expression {}", tinfo.val);
                        diags.err1("ERR_21", &msg, tinfo.span().clone());
                        diags.note0(
                            "ERR_22",
                            "At top-level, allowed expressions are 'section' and 'output'",
                        );
                        false
                    }
                };
        }

        if !result {
            bail!("AST construction failed");
        }

        let Some(output) = output else {
            diags.err0("ERR_7", "Missing output statement");
            bail!("AST construction failed");
        };

        // Check for const names that conflict with section names.
        for (const_name, const_span) in &const_names {
            if let Some(sec_item) = sections.get(const_name.as_str()) {
                let m = format!("Const name '{}' conflicts with a section name", const_name);
                diags.err2("ERR_26", &m, const_span.clone(), sec_item.src_loc.clone());
                result = false;
            }
        }

        // Check for region names that conflict with section or const names.
        for (reg_name, reg_entry) in &regions {
            if let Some(sec_item) = sections.get(reg_name.as_str()) {
                let m = format!("Region name '{}' conflicts with a section name", reg_name);
                diags.err2(
                    "ERR_43",
                    &m,
                    reg_entry.src_loc.clone(),
                    sec_item.src_loc.clone(),
                );
                result = false;
            }
            if let Some(const_span) = const_names.get(reg_name.as_str()) {
                let m = format!("Region name '{}' conflicts with a const name", reg_name);
                diags.err2("ERR_58", &m, reg_entry.src_loc.clone(), const_span.clone());
                result = false;
            }
        }

        // Validate section region references.
        // Track which regions are already bound to detect duplicate bindings.
        let mut bound_regions: HashMap<String, SourceSpan> = HashMap::new();
        for (sec_name, sec_entry) in &sections {
            let Some(ref reg_name) = sec_entry.region else {
                continue;
            };
            if let Some(reg_entry) = regions.get(reg_name) {
                if let Some(prev_span) = bound_regions.get(reg_name.as_str()) {
                    let m = format!(
                        "Section '{}' binds to region '{}' which is already bound to another section",
                        sec_name, reg_name
                    );
                    diags.err2("ERR_52", &m, sec_entry.src_loc.clone(), prev_span.clone());
                    result = false;
                } else {
                    bound_regions.insert(reg_name.clone(), sec_entry.src_loc.clone());
                }
                let _ = reg_entry;
            } else {
                let m = format!(
                    "Section '{}' references undeclared region '{}'",
                    sec_name, reg_name
                );
                diags.err1("ERR_51", &m, sec_entry.src_loc.clone());
                result = false;
            }
        }

        if !result {
            bail!("AST construction failed");
        }

        // Check for obj names that conflict with section, const, or region names.
        for (obj_name, obj_entry) in &obj_decls {
            if let Some(sec_item) = sections.get(obj_name.as_str()) {
                let m = format!("Obj name '{}' conflicts with a section name", obj_name);
                diags.err2("ERR_64", &m, obj_entry.src_loc.clone(), sec_item.src_loc.clone());
                result = false;
            }
            if let Some(const_span) = const_names.get(obj_name.as_str()) {
                let m = format!("Obj name '{}' conflicts with a const name", obj_name);
                diags.err2("ERR_65", &m, obj_entry.src_loc.clone(), const_span.clone());
                result = false;
            }
            if let Some(reg_entry) = regions.get(obj_name.as_str()) {
                let m = format!("Obj name '{}' conflicts with a region name", obj_name);
                diags.err2("ERR_66", &m, obj_entry.src_loc.clone(), reg_entry.src_loc.clone());
                result = false;
            }
        }

        let output_nid = output.nid;
        let mut ast_db = AstDb {
            sections,
            labels: HashMap::new(),
            output,
            pre_output_globals,
            post_output_globals,
            const_statements,
            const_names,
            regions,
            obj_decls,
        };

        if !ast_db.validate_section_name(0, output_nid, ast, diags) {
            bail!("AST construction failed");
        }

        if validate {
            let mut children = ast.children(output_nid);
            let sec_nid = children.next().unwrap();
            let sec_tinfo = ast.get_tinfo(sec_nid);
            let sec_str = sec_tinfo.val;

            let mut nested_sections: HashSet<String> = HashSet::new();
            nested_sections.insert(sec_str.to_string());
            let sec_body_nid = ast_db.sections.get(sec_str).unwrap().nid;
            let children = ast.children(sec_body_nid);

            for nid in children {
                result &= ast_db.validate_nesting_r(nid, ast, &mut nested_sections, diags);
            }

            if !result {
                bail!("AST construction failed");
            }
        }

        Ok(ast_db)
    }
}
