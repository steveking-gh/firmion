// Pre-layout region intersection data and validation.
//
// RegionDb::build runs once after IRDb, before the layout iterate loop begins.
// The build function walks irdb.ir_vec in order, tracking section nesting via
// a scope stack, and computes the EffectiveRegion for every section.  Because
// const_eval evaluates region addr and size before IRDb exists, the resulting
// intersections are stable and do not change across layout passes.
//

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::Diags;
use ir::{EffectiveRegion, IRKind, RegionProps};
use crate::IRDb;
use std::collections::{HashMap, HashSet};

#[allow(unused_imports)]
use tracing::{debug, error, info, trace, warn};

/// Pre-computed region intersection data, keyed by section name.
/// RegionDb::build produces this once after IRDb; LayoutPhase reads the map for
/// enforcement, and validate_section_regions reads the map for post-convergence
/// size checks.
pub struct RegionDb {
    pub effective_regions: HashMap<String, EffectiveRegion>,
}

impl RegionDb {
    /// Walk irdb.ir_vec once, computing the EffectiveRegion for every section.
    /// Returns None on any error.
    pub fn build(irdb: &IRDb, diags: &mut Diags) -> Option<RegionDb> {
        let mut effective_regions: HashMap<String, EffectiveRegion> = HashMap::new();
        // Scope stack: one entry per active section, innermost last.
        // None means the section has no region constraint.
        let mut effective_region_stack: Vec<Option<EffectiveRegion>> = Vec::new();
        // Tracks region-bound sections already entered -- detects re-use.
        let mut seen_region_sections: HashSet<String> = HashSet::new();
        let mut ok = true;

        // The only IR elements that matter for region intersection are
        // SectionStart and SectionEnd.
        for ir in &irdb.ir_vec {
            match ir.kind {
                IRKind::SectionStart => {
                    let sec_name = irdb.get_opnd_as_identifier(ir, 0);
                    trace!("RegionDb::build: Processing section '{}'", sec_name);
                    let parent_effective = effective_region_stack.last().and_then(|e| e.as_ref());
                    let direct_binding = irdb.region_for_section(sec_name);

                    // Region-bound sections anchor to a fixed address; re-inclusion conflicts.
                    if direct_binding.is_some()
                        && !seen_region_sections.insert(sec_name.to_string())
                    {
                        let msg = format!(
                            "Section '{}' is bound to a region and cannot be written more \
                             than once, since doing so produces an address conflict.",
                            sec_name
                        );
                        diags.err1("ERR_192", &msg, ir.src_loc.clone());
                        ok = false;
                    }

                    // Build region stack: inherit parent's, then append local region props, if any.
                    let mut region_stack: Vec<RegionProps> = parent_effective
                        .map(|e| e.region_stack.clone())
                        .unwrap_or_default();
                    if let Some(direct) = direct_binding {
                        region_stack.push(direct.clone());
                    }

                    // The effective regions depends on possible presence of both direct and inherited regions.
                    //
                    // * No direct nor inherited region: no effective region.
                    // * Inherited but no direct region: effective region is the inherited region.
                    // * Direct but no inherited region: effective region is the direct region.
                    // * Both direct and inherited region: effective region is the intersection.
                    //
                    let effective = match (
                        parent_effective.map(|e| &e.effective_region),
                        direct_binding,
                    ) {
                        (None, None) => None,
                        (Some(parent), None) => {
                            debug!(
                                "RegionDb::build: Section '{}' no direct region \
                                binding, inherits effective region '{}' [{:#X}, {:#X})",
                                sec_name,
                                parent.name,
                                parent.addr,
                                parent.addr + parent.size
                            );
                            Some(EffectiveRegion {
                                effective_region: parent.clone(),
                                region_stack,
                            })
                        }
                        (None, Some(direct)) => {
                            debug!(
                                "RegionDb::build: Section '{}' has direct region \
                                binding '{}' [{:#X}, {:#X}), inherits no effective region",
                                sec_name,
                                direct.name,
                                direct.addr,
                                direct.addr + direct.size
                            );

                            Some(EffectiveRegion {
                                effective_region: direct.clone(),
                                region_stack,
                            })
                        }
                        (Some(parent), Some(direct)) => {
                            debug!(
                                "RegionDb::build: Section '{}' has direct region \
                                binding '{}' [{:#X}, {:#X}), and also inherits effective region '{}' [{:#X}, {:#X})",
                                sec_name,
                                direct.name,
                                direct.addr,
                                direct.addr + direct.size,
                                parent.name,
                                parent.addr,
                                parent.addr + parent.size
                            );
                            match parent.intersect(direct) {
                                Some(intersection) => {
                                    // The section anchors to d.addr; d.addr must fall
                                    // within the parent constraint.
                                    if direct.addr < intersection.addr {
                                        let msg = format!(
                                            "Section '{}': region '{}' starts at {:#X}, which \
                                             is before the enclosing region '{}' start {:#X}. \
                                             The starting address of the directly bound region must lie within \
                                             the intersection with parent region(s) [{:#X}, {:#X}).",
                                            sec_name,
                                            direct.name,
                                            direct.addr,
                                            parent.name,
                                            parent.addr,
                                            intersection.addr,
                                            intersection.addr + intersection.size, // No overflow, bare addition is safe.
                                        );
                                        diags.err2(
                                            "ERR_191",
                                            &msg,
                                            direct.src_loc.clone(),
                                            parent.src_loc.clone(),
                                        );
                                        ok = false;
                                    }
                                    debug!(
                                        "RegionDb::build: Section '{}' effective region intersection \
                                            is '{}' [{:#X}, {:#X})",
                                        sec_name,
                                        intersection.name,
                                        intersection.addr,
                                        intersection.addr + intersection.size
                                    );
                                    Some(EffectiveRegion {
                                        effective_region: intersection,
                                        region_stack,
                                    })
                                }
                                None => {
                                    // Regions are completely disjoint.
                                    let msg = format!(
                                        "Section '{}': region '{}' [{:#X}, {:#X}) does not \
                                         intersect with enclosing region '{}' [{:#X}, {:#X}).",
                                        sec_name,
                                        direct.name,
                                        direct.addr,
                                        direct.addr + direct.size, // No overflow, bare addition is safe.
                                        parent.name,
                                        parent.addr,
                                        parent.addr + parent.size, // No overflow, bare addition is safe.
                                    );
                                    diags.err2(
                                        "ERR_190",
                                        &msg,
                                        direct.src_loc.clone(),
                                        parent.src_loc.clone(),
                                    );
                                    ok = false;
                                    // Fallback: push direct binding to keep the scope stack
                                    // structurally consistent for child sections.
                                    Some(EffectiveRegion {
                                        effective_region: direct.clone(),
                                        region_stack,
                                    })
                                }
                            }
                        }
                    };

                    if let Some(ref e) = effective {
                        effective_regions.insert(sec_name.to_string(), e.clone());
                    }
                    effective_region_stack.push(effective);
                }

                IRKind::SectionEnd => {
                    effective_region_stack.pop();
                }

                _ => {}
            }
        }

        if ok {
            Some(RegionDb { effective_regions })
        } else {
            None
        }
    }
}
