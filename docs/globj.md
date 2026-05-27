# Step-by-Step Implementation Plan: Glob Obj and default_pad_byte

This document details the step-by-step implementation plan to add:
1. Region-specific `default_pad_byte` property (Step 1 - Complete).
2. Glob matching, exclusions, and sorting DSL (Step 2 - Broken down into Steps 2A-2E).

Each step must compile warning-free and pass all existing and new unit/integration tests.

---

## Step 1: Region-specific `default_pad_byte` Support (Complete)

In this step, we add support for an optional `default_pad_byte` region property (which defaults to `0xFF` when a region is declared). When a section bound to a region uses `align` or `pad` without specifying a pad byte, the region's `default_pad_byte` is automatically applied. Sections not bound to any region fallback to `0x00`.

---

## Shared Glob DSL Library (`flexiglob`)

`flexiglob` is a standalone `#![no_std]` library located at `../flexiglob` (peer directory to Firmion). Firmion imports it as a workspace dependency. The canonical types, API, and tests live in [flexiglob/src/lib.rs](file:///c:/Users/kings/Documents/projects/flexiglob/src/lib.rs); do not duplicate type definitions in this document.

### Key API components

1. **Pattern parser**: `ParsedPattern::parse(input, is_valid_op)` parses nested operator pipelines such as `SORT_BY_ALIGNMENT(.text*)`. Wildcard token compilation happens during parsing. On failure returns `ParseError { kind: ParseErrorKind, span: Range<usize>, message: String }` where `span` is byte-offset relative to the input string.

2. **Thompson NFA matcher**: `wildcard_match(tokens, candidate)` matches a candidate string against compiled `MatchToken` tokens using a non-recursive NFA simulation. O(n x m) time, O(n) space. Supported tokens: `Char`, `AnyChar` (`?`), `AnySeq` (`**`), `AnySeqNoSeparator` (`*`), `Set` (`[chars]`), `NegatedSet` (`[^chars]`).

3. **Filesystem traversal hint**: `Globber::scan_hint() -> ScanHint { root: &str, recursive: bool }` extracts the literal path prefix before the first wildcard and indicates whether recursive traversal is required. Operator wrappers are transparent; the scan descends to the leaf pattern.

4. **Builder and execution engine**: `GlobberBuilder::new().with_operator(op).compile(pattern)` validates operator names against the registered set and returns a `Globber`. `Globber::run(candidates, get_name)` applies the pipeline and returns the filtered, ordered subset. The builder is constructed locally per call and discarded immediately; `ObjFileResolver` stores no builder state.

5. **No-std portability**: Fully `#![no_std]` (uses `alloc` only). Uses `BTreeMap`/`BTreeSet` throughout (no hash-based collections).

### Error mapping: `ParseError` to Firmion diagnostics

When `GlobberBuilder::compile()` returns `Err(ParseError)` during IRDb construction:

- Use `ObjProps.src_loc` as the source location anchor.
- Pass `ParseError.message` as the primary diagnostic message.
- Report via `diags.err1()` with one of the codes below.

Pattern validation fires in `irdb`, not in `ast`, keeping `flexiglob` out of the `ast` dependency graph.

**New error codes** (next available after ERR_232):

| Code | Condition |
| --- | --- |
| ERR_233 | Glob pattern syntax error (`EmptyPattern`, `MismatchedParentheses`, `UnexpectedParen`, `UnterminatedBracketSet`, `EmptyBrackets`, `UnexpectedTrailingCharacters`) |
| ERR_234 | Unknown glob operator name (`InvalidOperator`) |
| ERR_235 | LMA continuity violation during obj section packing (Step 2D) |

---

## Step 2: Backend Glob matching, Exclusions, and Sorting DSL

We break the complex "Step 2" backend glob matching, exclusions, and sorting DSL into 5 smaller sub-steps:

### Step 2A: Integrate Existing Flexiglob Crate

- **Goal:** Wire the existing `flexiglob` crate into the Firmion workspace. The library is already implemented and tested; this step only adds the dependency.
- **Files Modified:**
  - `[MODIFY]` [Cargo.toml](file:///c:/Users/kings/Documents/projects/firmion/Cargo.toml): Add `flexiglob = { path = "../flexiglob" }` to `[workspace.dependencies]`.
  - `[MODIFY]` [irdb/Cargo.toml](file:///c:/Users/kings/Documents/projects/firmion/irdb/Cargo.toml): Add `flexiglob = { workspace = true }`.
- **Verification:** `cargo build -p firmion-irdb` succeeds. `cargo test -p flexiglob` passes (existing unit tests in `flexiglob/src/lib.rs`).

### Step 2B: File Glob Expansion and Exclusions

- **Goal:** Traverse the filesystem to find matching object files.
- **Files Modified:**
  - `[MODIFY]` [ir/ir.rs](file:///c:/Users/kings/Documents/projects/firmion/ir/ir.rs): Update `ObjProps` to support single-string exclusions.
  - `[MODIFY]` [ast/ast.rs](file:///c:/Users/kings/Documents/projects/firmion/ast/ast.rs): Modify `parse_obj` to accept `"file_exclude"` and `"section_exclude"` properties.
  - `[MODIFY]` [const_eval/const_eval.rs](file:///c:/Users/kings/Documents/projects/firmion/const_eval/const_eval.rs): Update `evaluate_obj_props` and `resolve_obj_prop` to parse/validate single-string exclusions.
  - `[MODIFY]` [irdb/objfile.rs](file:///c:/Users/kings/Documents/projects/firmion/irdb/objfile.rs): Integrate file globbing, alphabetical sorting, and `file_exclude` filters. Construct `GlobberBuilder` and compile patterns locally; discard after use. On `Err(ParseError)`, emit ERR_233 or ERR_234 via `diags.err1()` using `ObjProps.src_loc`. Call `Globber::scan_hint()` to obtain `ScanHint { root, recursive }`; use `root` as the starting directory for filesystem traversal and `recursive` to decide whether to walk subdirectories. Collect all matching paths into a candidate slice, then pass to `Globber::run()` to apply the pattern and any pipeline operators.
- **Verification:** Integration tests verifying multiple file matching and file exclusions.

### Step 2C: Section Globbing, Exclusion, and Sorting

- **Goal:** Filter and sort sections within matched object files.
- **Files Modified:**
  - `[MODIFY]` [irdb/objfile.rs](file:///c:/Users/kings/Documents/projects/firmion/irdb/objfile.rs): Match sections within files, filter with `section_exclude`, and apply sorting operators.
- **New local types and operators** (all in `irdb/objfile.rs`):
  - `SectionCandidate`: local struct holding section name, alignment, and init priority. Used as the type parameter `T` for `GlobOperator<T>` and `Globber::run()`.
  - `SortByAlignmentOp`: implements `GlobOperator<SectionCandidate>`, operator name `"SORT_BY_ALIGNMENT"`. Sorts by alignment descending.
  - `SortByInitPriorityOp`: implements `GlobOperator<SectionCandidate>`, operator name `"SORT_BY_INIT_PRIORITY"`. Extracts the numeric suffix from section names (e.g. `.init_array.00100` -> 100) and sorts ascending.
  - Both operators are registered via `GlobberBuilder::with_operator()` inside `ObjFileResolver::resolve`.
- **Verification:** Integration tests verifying sorted sections, alignment sorting, priority sorting, and section exclusions.

### Step 2D: Layout Packing & Contiguity Validation

- **Goal:** Pack sections contiguously and perform LMA continuity checks.
- **Files Modified:**
  - `[MODIFY]` [ir/ir.rs](file:///c:/Users/kings/Documents/projects/firmion/ir/ir.rs): Update `ObjsecInfo` and `ObjsecSection` structures.
  - `[MODIFY]` [irdb/objfile.rs](file:///c:/Users/kings/Documents/projects/firmion/irdb/objfile.rs): Sequential offsets computation and LMA continuity assertion (raising `ERR_235`).
- **Verification:** Integration tests verifying alignment packing offsets and disjoint LMA error raising.

### Step 2E: Sequential Section Writing

- **Goal:** Write packed section contents into the final binary.
- **Files Modified:**
  - `[MODIFY]` [exec_phase/exec_phase.rs](file:///c:/Users/kings/Documents/projects/firmion/exec_phase/exec_phase.rs): Iterate and write matched section slices contiguously.
- **Verification:** Integration tests verifying correct final binary output with multi-file section dumps.
