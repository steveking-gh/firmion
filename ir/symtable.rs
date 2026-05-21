// Symbol table for Firmion compile-time const variables.
//
// SymbolTable tracks every const identifier from its declaration through its
// use sites, enabling unused-const warnings and — in a future phase — proper
// declare-before-assign semantics for if/else conditional const assignment.
//
// Usage tracking:
//   mark_used()   sets the used flag on any entry.
//   warn_unused() emits ERR_229 for every entry with Some(value) whose used
//   flag is still false at the end of IRDb construction.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::{Diags, SourceSpan};
use crate::ParameterValue;
use std::collections::HashMap;

/// One entry in the symbol table.
pub struct ConstEntry {
    /// The resolved value, or None if declared but not yet assigned.
    pub value: Option<ParameterValue>,
    /// True once the value has been referenced by any expression or operand.
    pub used: bool,
    /// Source location of the declaration, if it came from a source file.
    /// None for command-line -D defines which have no source location.
    pub decl_loc: Option<SourceSpan>,
}

/// Flat symbol table for compile-time const values.
///
/// Phase 2 will extend this with scope push/pop for if/else blocks.
pub struct SymbolTable {
    entries: HashMap<String, ConstEntry>,
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            entries: HashMap::new(),
        }
    }

    /// Define a const with a resolved value.  `loc` is the declaration site
    /// for unused-const warning messages; pass `None` for command-line `-D`
    /// defines that have no source location.
    pub fn define(&mut self, name: String, value: ParameterValue, loc: Option<SourceSpan>) {
        self.entries.insert(
            name,
            ConstEntry {
                value: Some(value),
                used: false,
                decl_loc: loc,
            },
        );
    }

    /// Declare a const with no value (for `const NAME;` deferred assignment).
    /// Ignored if the name is already in the table (e.g. from a `-D` command-line define).
    pub fn declare(&mut self, name: String, loc: SourceSpan) {
        self.entries.entry(name).or_insert(ConstEntry {
                    value: None,
                    used: false,
                    decl_loc: Some(loc),
                });
    }

    /// Assign a value to a previously-declared const (bare assignment inside an if/else body).
    /// Returns false and emits ERR_230 if the name was not declared.
    pub fn assign(
        &mut self,
        name: &str,
        value: ParameterValue,
        loc: &SourceSpan,
        diags: &Diags,
    ) -> bool {
        if let Some(entry) = self.entries.get_mut(name) {
            if entry.value.is_some() {
                let Some(decl_loc) = entry.decl_loc.as_ref() else {
                    return true;
                };
                let m = format!("Const '{}' cannot be assigned more than once.", name);
                diags.err2(
                    "ERR_231",
                    &m,
                    loc.clone(),
                    decl_loc.clone(),
                );
                return false;
            }
            entry.value = Some(value);
            true
        } else {
            let m = format!(
                "Assignment to '{}' which was not pre-declared with 'const {};'.",
                name, name
            );
            diags.err1("ERR_230", &m, loc.clone());
            false
        }
    }

    /// Returns a reference to the value if the const has one, or `None`
    /// if the name is unknown or declared but not yet assigned.
    pub fn get(&self, name: &str) -> Option<&ParameterValue> {
        self.entries.get(name)?.value.as_ref()
    }

    /// Mark the named const as used.  Every call site calls `get()` first and
    /// only calls this function on success, so the name is guaranteed to be
    /// present.  Panics if the name is missing — that would indicate a bug at
    /// the call site.
    pub fn mark_used(&mut self, name: &str) {
        self.entries
            .get_mut(name)
            .unwrap_or_else(|| panic!("mark_used: const '{}' is not in the symbol table", name))
            .used = true;
    }

    /// Returns true if the name is present in the table (any state).
    pub fn contains_key(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Returns the resolved value for a name, if it has one.
    pub fn get_value(&self, name: &str) -> Option<ParameterValue> {
        self.entries.get(name)?.value.clone()
    }

    /// Iterates over all entries with a value, yielding `(name, &ParameterValue, used)`.
    pub fn iter_defined_with_used(&self) -> impl Iterator<Item = (&str, &ParameterValue, bool)> {
        self.entries
            .iter()
            .filter_map(|(k, e)| e.value.as_ref().map(|v| (k.as_str(), v, e.used)))
    }

    /// Emit `ERR_229` warnings for every const with a value that was never used.
    /// Call this after all operand substitution is complete.
    pub fn warn_unused(&self, diags: &Diags) {
        for (name, entry) in &self.entries {
            if entry.value.is_some() && !entry.used {
                let m = format!("Const '{}' is defined but never used.", name);
                diags.warn_opt("ERR_229", &m, entry.decl_loc.clone());
            }
        }
    }
}
