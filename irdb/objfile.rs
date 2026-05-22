// Object file parsing and section resolution for firmion.
//
// This crate isolates all interaction with the `object` crate.  ObjsecProps
// and the LMA-fill helpers are private implementation details.  The only public
// surface is ObjFileResolver, which accepts a reference to the obj_props map
// produced by const_eval and resolves individual obj sections on demand,
// caching each parsed file so multi-section references to the same file pay
// the I/O cost only once.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::{Diags, SourceSpan};
use ir::{ObjProps, ObjsecInfo};
use object::{Object, ObjectSection};
use std::{collections::HashMap, fs};

// Raw per-section data extracted from one parse of an object file.
// lma starts as None; fill_lma populates it from PT_LOAD segments.
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

/// Resolves obj sections on demand.  Holds a reference to the obj_props map
/// from const_eval and a per-file parse cache so each object file is opened
/// and parsed at most once regardless of how many obj declarations reference it.
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

    // Parse file_path and insert its section map into self.parsed.
    // Does nothing if the file is already cached.
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

    /// Resolve the named obj declaration to an ObjSectionInfo.
    /// Parses the backing object file on first access; subsequent references to
    /// the same file use the cached parse result.  Returns None and emits a
    /// diagnostic on any failure.
    pub fn resolve(
        &mut self,
        obj_name: &str,
        use_loc: &SourceSpan,
        diags: &mut Diags,
    ) -> Option<ObjsecInfo> {
        let props = match self.obj_props.get(obj_name) {
            Some(p) => p,
            None => {
                let m = format!("Unknown obj name '{}'", obj_name);
                diags.err1("ERR_117", &m, use_loc.clone());
                return None;
            }
        };
        let file_path = props.file.clone();
        let objsec_name = props.name.clone();
        let decl_loc = props.src_loc.clone();

        if !self.cache_file(&file_path, use_loc, &decl_loc, diags) {
            return None;
        }
        let objsec_map = self.parsed.get(&file_path).unwrap();
        let Some(raw) = objsec_map.get(&objsec_name) else {
            let m = format!(
                "Objsec '{}' not found in '{}', or objsec has no file data \
                 (e.g. zero-initialized sections cannot be copied).",
                objsec_name, file_path
            );
            diags.err2("ERR_119", &m, use_loc.clone(), decl_loc.clone());
            return None;
        };
        Some(ObjsecInfo {
            file: file_path,
            name: objsec_name,
            file_offset: raw.file_offset,
            size: raw.size,
            align: raw.align,
            vma: raw.vma,
            lma: raw.lma.unwrap(),
            src_loc: use_loc.clone(),
        })
    }
}
