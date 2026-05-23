# Step-by-Step Implementation Plan: Glob Obj, Array Syntax, and default_pad_byte

This document details the step-by-step implementation plan to add:
1. Region-specific `default_pad_byte` property (Step 1).
2. Glob matching, exclusions, and sorting DSL (Step 2).
3. Array/list syntax support (Step 3).

Each step must compile warning-free and pass all existing and new unit/integration tests.

---

## Step 1: Region-specific `default_pad_byte` Support

In this step, we add support for an optional `default_pad_byte` region property (which defaults to `0xFF` when a region is declared). When a section bound to a region uses `align` or `pad` without specifying a pad byte, the region's `default_pad_byte` is automatically applied. Sections not bound to any region fallback to `0x00`.

### Proposed Changes

#### [MODIFY] [ast/ast.rs](file:///c:/Users/kings/Documents/projects/firmion/ast/ast.rs)
- In `parse_region_contents`, support parsing the `"default_pad_byte"` property and validate it as a duplicate check. Update unknown property error message to include `"default_pad_byte"`.

#### [MODIFY] [ir/ir.rs](file:///c:/Users/kings/Documents/projects/firmion/ir/ir.rs)
- Update `RegionProps` to hold the default padding byte:
  ```rust
  pub struct RegionProps {
      pub addr: u64,
      pub size: u64,
      pub name: String,
      pub default_pad_byte: u8,
      pub src_loc: SourceSpan,
  }
  ```

#### [MODIFY] [const_eval/const_eval.rs](file:///c:/Users/kings/Documents/projects/firmion/const_eval/const_eval.rs)
- Update `evaluate_regions` to parse, validate (checking that it is a numeric value $\le 255$), and store `default_pad_byte` in `RegionProps` (defaulting to `0xFF`).

#### [MODIFY] [const_eval/linearizer.rs](file:///c:/Users/kings/Documents/projects/firmion/const_eval/linearizer.rs)
- Modify statement lowering for `Align` and padding directives:
  - If no explicit pad byte is specified, instead of synthesizing a literal `"0"` operand, synthesize a placeholder Ref operand named `"__default_pad_byte"`.

#### [MODIFY] [irdb/irdb.rs](file:///c:/Users/kings/Documents/projects/firmion/irdb/irdb.rs)
- In `IRDb::process_linear_ir`, track the active section using a stack of `SectionStart` and `SectionEnd` opcodes.
- When traversing IR instructions, if an operand is a DeferredRef to `"__default_pad_byte"`, resolve it:
  - Get the active section.
  - Find the bound region for the section.
  - If bound to a region, look up its `default_pad_byte` from `region_props` (otherwise fallback to `0x00`).
  - Overwrite the operand value in `self.parms` with the resolved integer.

### Verification (Step 1)
- Write new integration tests in `tests/` verifying:
  - An `align` directive in a section bound to a region without `default_pad_byte` uses `0xFF`.
  - An `align` directive in a section bound to a region with `default_pad_byte = 0xAA` uses `0xAA`.
  - An `align` directive in a section not bound to a region uses `0x00`.
  - An explicit pad byte overrides the default (e.g., `align 8, 0xEE;` still pads with `0xEE`).

---

## Step 2: Backend Glob matching, Exclusions, and Sorting DSL

In this step, we implement glob matching across files/sections, exclusion filters, sorting directives, sequential layout packing, contiguity checks, and multi-section writing. In this phase, `file` and `section` properties accept single quoted strings only.

### Proposed Changes

#### [MODIFY] [ir/ir.rs](file:///c:/Users/kings/Documents/projects/firmion/ir/ir.rs)
- Update `ObjProps` to support single-string exclusions:
  ```rust
  pub struct ObjProps {
      pub file: String,
      pub file_exclude: Option<String>,
      pub name: String, // represents section pattern
      pub section_exclude: Option<String>,
      pub src_loc: SourceSpan,
  }
  ```
- Update `ObjsecInfo` to support multiple matching sections:
  ```rust
  pub struct ObjsecSection {
      pub file: String,
      pub name: String,
      pub file_offset: u64,
      pub size: u64,
      pub align: u64,
      pub offset_in_obj: u64, // Relative offset within the packed obj block
  }

  pub struct ObjsecInfo {
      pub name: String,
      pub sections: Vec<ObjsecSection>,
      pub size: u64,    // Total packed size including alignment padding
      pub align: u64,   // Largest alignment requirement of all sections
      pub lma: u64,     // LMA of the first section in the sorted sequence
      pub vma: u64,     // VMA of the first section in the sorted sequence
      pub src_loc: SourceSpan,
  }
  ```

#### [MODIFY] [ast/ast.rs](file:///c:/Users/kings/Documents/projects/firmion/ast/ast.rs)
- Modify `parse_obj` to accept `"file_exclude"` and `"section_exclude"` properties.

#### [MODIFY] [const_eval/const_eval.rs](file:///c:/Users/kings/Documents/projects/firmion/const_eval/const_eval.rs)
- Update `evaluate_obj_props` and `resolve_obj_prop` to parse and validate `file_exclude` and `section_exclude` (resolving them as optional single strings).

#### [MODIFY] [irdb/objfile.rs](file:///c:/Users/kings/Documents/projects/firmion/irdb/objfile.rs)
- Implement file glob expansion to expand `file` pattern into a list of absolute/relative file paths. Sort matched files alphabetically.
- Apply `file_exclude` pattern to filter out files.
- Parse the sorting directives (`SORT`, `SORT_BY_NAME`, `SORT_BY_ALIGNMENT`, `SORT_BY_INIT_PRIORITY`, and `REVERSE`) nested within `section` matching strings.
- For each matching file, scan all sections, apply glob name matching, filter using `section_exclude` patterns, and apply the parsed sorting operators.
- Pack the resolved sections sequentially starting at offset 0:
  - For $i > 1$:
    $$offset\_in\_obj_i = \text{align\_up}(offset\_in\_obj_{i-1} + size_{i-1}, align_i)$$
  - Compute the total packed `size` as $offset\_in\_obj_n + size_n$.
  - Compute the `align` as the maximum alignment requirement of all sections.
  - Set `lma` and `vma` to the load/virtual addresses of the first section.
- Implement the LMA contiguity check:
  - Verify that for each section $S_{i+1}$ (where $LMA \neq 0$):
    $$LMA_{i+1} == \text{align\_up}(LMA_i + size_i, align_{i+1})$$
  - Emit a contiguity error if the validation fails (checking literal patterns vs. glob wildcards).

#### [MODIFY] [exec_phase/exec_phase.rs](file:///c:/Users/kings/Documents/projects/firmion/exec_phase/exec_phase.rs)
- Modify `execute_wrobj` to iterate over `info.sections` sequentially:
  - Write padding (zero bytes) up to `sec.offset_in_obj` relative to the block start.
  - Open `sec.file`, seek to `sec.file_offset`, and copy `sec.size` bytes into the output buffer.

### Verification (Step 2)
- Write new integration tests in `tests/` verifying:
  - Single-string glob matching of multiple sections within an ELF file.
  - Glob matching across files, exclusions of files/sections, and sorting operators.
  - Sequential alignment packing and LMA contiguity checks (raising errors for disjoint segments).

---

## Step 3: Array / List Syntax Support

In this step, we add bracketed list syntax `[...]` to the language to support defining lists of strings for `file`, `section`, `file_exclude`, and `section_exclude` properties.

### Proposed Changes

#### [MODIFY] [ast/lexer.rs](file:///c:/Users/kings/Documents/projects/firmion/ast/lexer.rs)
- Modify `scan_operator` to scan single-character tokens `[` and `]` and map them to `LexToken::OpenBracket` and `LexToken::CloseBracket`.

#### [MODIFY] [ast/ast.rs](file:///c:/Users/kings/Documents/projects/firmion/ast/ast.rs)
- Add `OpenBracket`, `CloseBracket`, and a synthetic token `Array` to the `LexToken` enum.
- In `parse_pratt`, add support for parsing bracketed lists (`[...]`):
  - Skip `[`, create an AST node of type `LexToken::Array`.
  - Parse comma-separated expression elements recursively using `parse_pratt(0, ...)`.
  - Expect and consume the closing `]` token.
- In `parse_obj`, replace the strict `parse_leaf` call for property values with `parse_pratt(0, &mut expr_nid, diags)` to allow evaluating bracketed array expressions, constant identifiers, or string literals.

#### [MODIFY] [ir/ir.rs](file:///c:/Users/kings/Documents/projects/firmion/ir/ir.rs)
- Add `Array(Vec<ParameterValue>)` to the `ParameterValue` enum.
- Update `ObjProps` to store `Vec<String>` lists:
  ```rust
  pub struct ObjProps {
      pub files: Vec<String>,
      pub file_excludes: Vec<String>,
      pub sections: Vec<String>,
      pub section_excludes: Vec<String>,
      pub src_loc: SourceSpan,
  }
  ```

#### [MODIFY] [const_eval/const_eval.rs](file:///c:/Users/kings/Documents/projects/firmion/const_eval/const_eval.rs)
- Update `eval_expr_tree` to handle `LexToken::Array` by recursively evaluating all child nodes and collecting them into a `ParameterValue::Array`.
- Add a helper `resolve_obj_prop_list` that evaluates property expression trees:
  - If the expression is a string literal (or references a string constant), returns a single-item `Vec<String>`.
  - If the expression is an array, validates that all elements are string values and returns them as a `Vec<String>`.
  - Absent properties return an empty `Vec`.
- Update `evaluate_obj_props` and `resolve_obj_prop` to populate the new vector-based `ObjProps`.

#### [MODIFY] [irdb/objfile.rs](file:///c:/Users/kings/Documents/projects/firmion/irdb/objfile.rs)
- Adapt `ObjFileResolver::resolve` to loop over all files in `props.files` and matching section pattern lists.

### Verification (Step 3)
- Write new integration tests in `tests/` verifying:
  - Multi-item lists in `file` and `section` (e.g. `file = ["a.elf", "b.elf"]`).
  - Multi-item lists in `file_exclude` and `section_exclude`.
  - Nested expressions and constant identifiers evaluated inside arrays.
