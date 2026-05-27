// Object file parsing and section resolution for firmion.
//
// ObjFileResolver accepts the obj_props map from const_eval and resolves each
// obj declaration to the ordered list of ObjsecInfo values that wrobj will
// write.  File and section names are treated as flexiglob patterns, so a
// literal path (no wildcards) behaves identically to the pre-glob single-file
// form.  Pattern validation errors are reported as ERR_233 (syntax) or
// ERR_234 (unknown operator).
//
// For each resolved obj the sections are sorted by the glob pipeline
// (SORT_BY_ALIGNMENT, SORT_BY_INIT_PRIORITY, etc.) and packed contiguously
// in that order; LMA continuity is validated as ERR_235.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::{Diags, SourceSpan};
use flexiglob::{GlobOperator, GlobberBuilder, ParseErrorKind};
use ir::{ObjProps, ObjsecInfo};
use object::{Object, ObjectSection};
use std::{collections::HashMap, fs};

// Raw section data extracted from one parse of an object file

struct ObjsecProps {
    file_offset: u64,
    size: u64,
    align: u64,
    vma: u64,
    lma: Option<u64>,
}

fn fill_lma<Elf>(
    elf: &object::read::elf::ElfFile<'_, Elf>,
    objsec_map: &mut HashMap<String, ObjsecProps>,
) where
    Elf: object::read::elf::FileHeader,
    Elf::Word: Into<u64>,
{
    use object::read::elf::ProgramHeader as _;
    let endian = elf.endian();
    for phdr in elf.elf_program_headers() {
        if phdr.p_type(endian) != object::elf::PT_LOAD {
            continue;
        }
        let seg_vma: u64 = phdr.p_vaddr(endian).into();
        let seg_pma: u64 = phdr.p_paddr(endian).into();
        let seg_end: u64 = seg_vma + Into::<u64>::into(phdr.p_memsz(endian));
        for props in objsec_map.values_mut() {
            if props.lma.is_some() {
                continue;
            }
            if props.vma >= seg_vma && props.vma < seg_end {
                props.lma = Some(seg_pma + (props.vma - seg_vma));
            }
        }
    }
    for props in objsec_map.values_mut() {
        if props.lma.is_none() {
            props.lma = Some(props.vma);
        }
    }
}

fn compute_lma_from_segments(
    obj: &object::File<'_>,
    objsec_map: &mut HashMap<String, ObjsecProps>,
) {
    match obj {
        object::File::Elf32(elf) => fill_lma(elf, objsec_map),
        object::File::Elf64(elf) => fill_lma(elf, objsec_map),
        _ => {}
    }
}

// ── Section candidates used as the flexiglob type parameter ───────────────────

// Holds all data needed to build an ObjsecInfo plus the fields used by the
// sorting operators.
struct SectionCandidate {
    name: String,
    file: String,
    file_offset: u64,
    size: u64,
    align: u64,
    vma: u64,
    lma: u64,
}

// ── Sorting operators ──────────────────────────────────────────────────────────

struct SortByAlignmentOp;

impl GlobOperator<SectionCandidate> for SortByAlignmentOp {
    fn name(&self) -> &str { "SORT_BY_ALIGNMENT" }
    fn apply(&self, candidates: &mut Vec<&SectionCandidate>) {
        // Stable sort, descending alignment (largest alignment first).
        candidates.sort_by(|a, b| b.align.cmp(&a.align));
    }
}

struct SortByInitPriorityOp;

impl GlobOperator<SectionCandidate> for SortByInitPriorityOp {
    fn name(&self) -> &str { "SORT_BY_INIT_PRIORITY" }
    fn apply(&self, candidates: &mut Vec<&SectionCandidate>) {
        // Extract the trailing numeric suffix (e.g. ".init_array.00100" -> 100).
        // Sections without a numeric suffix sort after those that have one.
        candidates.sort_by_key(|c| init_priority(&c.name));
    }
}

fn init_priority(name: &str) -> u64 {
    name.rsplit('.').next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(u64::MAX)
}

// ── Glob error -> Firmion diagnostic ──────────────────────────────────────────

fn emit_glob_error(e: &flexiglob::ParseError, src_loc: &SourceSpan, diags: &mut Diags) {
    let code = if matches!(e.kind, ParseErrorKind::InvalidOperator(_)) {
        "ERR_234"
    } else {
        "ERR_233"
    };
    diags.err1(code, &e.message, src_loc.clone());
}

// ── ObjFileResolver ────────────────────────────────────────────────────────────

/// Resolves obj declarations to ordered lists of ObjsecInfo.
/// Each file is parsed at most once; subsequent references use the cached result.
pub struct ObjFileResolver<'a> {
    obj_props: &'a HashMap<String, ObjProps>,
    parsed: HashMap<String, HashMap<String, ObjsecProps>>,
}

impl<'a> ObjFileResolver<'a> {
    pub fn new(obj_props: &'a HashMap<String, ObjProps>) -> Self {
        Self {
            obj_props,
            parsed: HashMap::new(),
        }
    }

    // Parse file_path and cache its section map.  No-op if already cached.
    fn cache_file(
        &mut self,
        file_path: &str,
        use_loc: &SourceSpan,
        decl_loc: &SourceSpan,
        diags: &mut Diags,
    ) -> bool {
        if self.parsed.contains_key(file_path) {
            return true;
        }
        let bytes = match fs::read(file_path) {
            Ok(b) => b,
            Err(e) => {
                let m = format!("Cannot read object file '{}': {}", file_path, e);
                diags.err2("ERR_118", &m, use_loc.clone(), decl_loc.clone());
                return false;
            }
        };
        let obj = match object::File::parse(bytes.as_slice()) {
            Ok(o) => o,
            Err(e) => {
                let m = format!(
                    "'{}' is not a recognized object file format: {}",
                    file_path, e
                );
                diags.err2("ERR_120", &m, use_loc.clone(), decl_loc.clone());
                return false;
            }
        };
        let mut objsec_map: HashMap<String, ObjsecProps> = HashMap::new();
        for section in obj.sections() {
            if let Ok(name) = section.name()
                && let Some((file_offset, size)) = section.file_range()
            {
                objsec_map.insert(
                    name.to_string(),
                    ObjsecProps {
                        file_offset,
                        size,
                        align: section.align(),
                        vma: section.address(),
                        lma: None,
                    },
                );
            }
        }
        compute_lma_from_segments(&obj, &mut objsec_map);
        self.parsed.insert(file_path.to_string(), objsec_map);
        true
    }

    /// Resolve the named obj declaration to an ordered list of ObjsecInfo.
    /// Returns None and emits a diagnostic on any failure, including no matches.
    pub fn resolve(
        &mut self,
        obj_name: &str,
        use_loc: &SourceSpan,
        diags: &mut Diags,
    ) -> Option<Vec<ObjsecInfo>> {
        let props = match self.obj_props.get(obj_name) {
            Some(p) => p,
            None => {
                let m = format!("Unknown obj name '{}'", obj_name);
                diags.err1("ERR_117", &m, use_loc.clone());
                return None;
            }
        };

        // Clone props fields needed after the mutable self borrows below.
        let file_pat = props.file.clone();
        let sec_pat  = props.name.clone();
        let file_excl_pat = props.file_exclude.clone();
        let sec_excl_pat  = props.section_exclude.clone();
        let decl_loc = props.src_loc.clone();

        // ── Step 1: expand file glob ───────────────────────────────────────────
        let file_builder = GlobberBuilder::<String>::new();
        let file_globber = match file_builder.compile(&file_pat) {
            Ok(g) => g,
            Err(ref e) => { emit_glob_error(e, &decl_loc, diags); return None; }
        };
        let matched_files = file_globber.run_fs();

        // Apply file exclusion if specified.
        let matched_files: Vec<String> = if file_excl_pat.is_empty() {
            matched_files
        } else {
            let excl_builder = GlobberBuilder::<String>::new();
            let excl_globber = match excl_builder.compile(&file_excl_pat) {
                Ok(g) => g,
                Err(ref e) => { emit_glob_error(e, &decl_loc, diags); return None; }
            };
            // Collect into owned set so the borrow of matched_files ends before into_iter().
            let excluded: std::collections::HashSet<String> =
                excl_globber.run(&matched_files, |s| s.as_str())
                    .into_iter().cloned().collect();
            matched_files.into_iter().filter(|f| !excluded.contains(f)).collect()
        };

        if matched_files.is_empty() {
            let m = format!(
                "obj '{}': file pattern '{}' matched no files.",
                obj_name, file_pat
            );
            diags.err2("ERR_236", &m, use_loc.clone(), decl_loc.clone());
            return None;
        }

        // ── Step 2: for each matched file, expand section glob ─────────────────
        let mut sec_builder = GlobberBuilder::new();
        if let Err(ref e) = sec_builder.register_operator(SortByAlignmentOp) {
            emit_glob_error(e, &decl_loc, diags);
            return None;
        }
        if let Err(ref e) = sec_builder.register_operator(SortByInitPriorityOp) {
            emit_glob_error(e, &decl_loc, diags);
            return None;
        }
        let sec_globber = match sec_builder.compile(&sec_pat) {
            Ok(g) => g,
            Err(ref e) => { emit_glob_error(e, &decl_loc, diags); return None; }
        };
        let sec_excl_globber_opt: Option<flexiglob::Globber<'_, SectionCandidate>> =
            if sec_excl_pat.is_empty() {
                None
            } else {
                match sec_builder.compile(&sec_excl_pat) {
                    Ok(g) => Some(g),
                    Err(ref e) => { emit_glob_error(e, &decl_loc, diags); return None; }
                }
            };

        let mut all_infos: Vec<ObjsecInfo> = Vec::new();

        for file_path in matched_files {
            if !self.cache_file(&file_path, use_loc, &decl_loc, diags) {
                return None;
            }
            let objsec_map = self.parsed.get(&file_path).unwrap();

            // Build candidates from all sections that have file data.
            let mut section_candidates: Vec<SectionCandidate> = objsec_map
                .iter()
                .map(|(name, raw)| SectionCandidate {
                    name: name.clone(),
                    file: file_path.clone(),
                    file_offset: raw.file_offset,
                    size: raw.size,
                    align: raw.align,
                    vma: raw.vma,
                    lma: raw.lma.unwrap(),
                })
                .collect();
            // Stable pre-sort by name so results are deterministic before operators run.
            section_candidates.sort_by(|a, b| a.name.cmp(&b.name));

            let mut matched_secs = sec_globber.run(&section_candidates, |c| c.name.as_str());

            // Apply section exclusion if specified.
            if let Some(ref excl_g) = sec_excl_globber_opt {
                let excluded = excl_g.run(&section_candidates, |c| c.name.as_str());
                let excl_names: std::collections::HashSet<&str> =
                    excluded.iter().map(|c| c.name.as_str()).collect();
                matched_secs.retain(|c| !excl_names.contains(c.name.as_str()));
            }

            for sec in matched_secs {
                all_infos.push(ObjsecInfo {
                    file: sec.file.clone(),
                    name: sec.name.clone(),
                    file_offset: sec.file_offset,
                    size: sec.size,
                    align: sec.align,
                    vma: sec.vma,
                    lma: sec.lma,
                    src_loc: use_loc.clone(),
                });
            }
        }

        if all_infos.is_empty() {
            let m = format!(
                "Objsec pattern '{}' matched no sections in the matched files.",
                sec_pat
            );
            diags.err2("ERR_119", &m, use_loc.clone(), decl_loc.clone());
            return None;
        }

        // ── Step 3: LMA continuity check (ERR_235) ────────────────────────────
        // Adjacent sections must have contiguous LMA addresses to guarantee the
        // packed output is a valid contiguous image slice.
        for i in 1..all_infos.len() {
            let prev = &all_infos[i - 1];
            let curr = &all_infos[i];
            if prev.lma + prev.size != curr.lma {
                let m = format!(
                    "obj '{}': LMA of section '{}' in '{}' ({:#X}) does not follow \
                     section '{}' in '{}' ({:#X} + {} = {:#X}). \
                     Sections must be LMA-contiguous.",
                    obj_name,
                    curr.name, curr.file, curr.lma,
                    prev.name, prev.file, prev.lma, prev.size, prev.lma + prev.size,
                );
                diags.err1("ERR_235", &m, use_loc.clone());
                return None;
            }
        }

        Some(all_infos)
    }
}
