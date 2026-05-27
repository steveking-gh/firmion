# Update Tracker

Meaningful changes only. Format: `YYYY-MM-DD — title — brief description`.

---

## 2026-04-11 — Top-level if/else section support

**Strategy B — eval_ast_condition refactor**
Eliminated `eval_ast_expr` (~140 lines). `eval_ast_condition` now reuses
`Linearizer::record_expr_r` + `ConstIR::eval_const_expr_r`. Symbol table
parameter changed from `&SymbolTable` to `&mut SymbolTable` throughout
(`const_eval`, `prune`, `process`).

**unreachable! conversion**
Three structural invariant `bail!` calls in `prune/prune.rs` converted to
`unreachable!()`. User-reachable failure paths retain `bail!`.

**Top-level if/else section support**
Sections may now be defined inside top-level `if/else` blocks.

Changes:

- `ast/ast.rs`: `ParseIfContext` enum (`TopLevel | Section`) threads through
  `parse_if_r` and `parse_if_body_r`. `TopLevel` context allows `Section`
  token in if body. `AstDb::new` gains `validate: bool` parameter; nesting
  validation skipped when `false`.
- `prune/prune.rs`: Removed `AstDb` dependency. Added `keep: fn(LexToken)->bool`
  filter. Root-level prune keeps `Section | If` only; section-body prune keeps
  all. Two-step prune: root if/else first, then section-body if/else.
- `process/process.rs`: Two AstDb passes — `validate=false` before const_eval,
  `validate=true` on pruned AST before LayoutDb.
- `layoutdb/fuzz/fuzz_targets/fuzz_target_1.rs`: Updated `AstDb::new` call to
  three-arg signature.
- 4 new test fixtures: `toplevel_if_section_true.firm`,
  `toplevel_if_section_false.firm`, `toplevel_if_else_section.firm`,
  `toplevel_if_nested_if.firm`.
- 4 new integration tests in `tests/integration.rs`.
- All 286 tests pass.

**AI context files created**
`docs/ai/01-meta.yaml`, `02-system.yaml`, `03-structure.yaml`, `04-memory.yaml`,
`05-update-tracker.md` (this file). Added `.claude/CLAUDE.md` workspace
instructions.

---

## 2026-04-12 -- engine.rs refactor and correctness fixes

**Location::advance extraction**
Extracted `Location::advance(&mut self, sz, src_loc, diags) -> bool` to
deduplicate checked-arithmetic counter advance across all four iterate helpers
(`iterate_wrs`, `iterate_wrext`, `iterate_wrx`, `iterate_wrf`).  All write
overflow paths now emit ERR_159 (file-offset overflow) or ERR_165 (absolute
address overflow).  Retired per-helper codes ERR_163, ERR_177, ERR_162.

**wrext overflow fix**
`iterate_wrext` now uses checked arithmetic via `Location::advance`.  Added
`MockHugeExt` (size = usize::MAX) in `ext/test_mocks.rs` and integration test
`wrext_overflow`.

**is_none + unwrap antipattern removal**
Replaced eight `is_none()`/`unwrap()` pairs across `engine.rs`, `ast/ast.rs`,
`irdb/irdb.rs`, and `symtable/symtable.rs` with `let Some(x) = ... else`
let-else idiom.

**execute_assert cleanup**
Removed the unnecessary `mut result` variable; failure branch now returns
`Err(...)` directly; success path returns `Ok(())`.

**build_dispatches: checked arithmetic + diags**
Added `diags: &mut Diags` parameter to `build_dispatches`.  Replaced two
`saturating_add` calls with `checked_add`; overflow emits ERR_177 (section
address) or ERR_178 (label address).

**execute_extensions: O(N^2) -> O(N) lookups**
Built two maps before the extension execution loop:

- `operand_consumer: HashMap<usize, usize>` -- resolves each extension call's
  consuming IR in O(1) instead of scanning ir_vec per call.
- `sec_dispatch_map: HashMap<&str, Vec<usize>>` -- resolves section dispatch
  for ExtensionCallSection in O(1) instead of two linear scans per call.

**CLAUDE.md / docs/ai ingestion**
Renamed `.claude/claude.md` to `.claude/CLAUDE.md`.  Corrected `repo_root` in
`03-structure.yaml` from Windows to WSL path.  Updated test count to 291.

All 291 tests pass.

---

## 2026-04-13 -- ExtArg typed extension API (Steps 1-4)

**FirmionRangedExtension removed; ExtArg introduced**
Eliminated the two-trait extension design (`FirmionExtension` + `FirmionRangedExtension`)
in favor of a single typed-argument API.

`ExtArg<'a>` enum added to `firmion_extension`:

- `Int(u64)` -- numeric arg
- `Str(&'a str)` -- quoted string arg
- `Section { start: u64, len: u64, data: &'a [u8] }` -- section name arg,
  resolved by the engine to a zero-copy mmap slice

All extensions now implement `FirmionExtension::execute<'a>(&self, args: &[ExtArg<'a>], out: &mut [u8])`.
Section-bound extensions (formerly FirmionRangedExtension) receive image data via
`args[0]` as `ExtArg::Section` when called with the section-name form.

**IRKind::ExtensionCallRanged removed**
Only `ExtensionCall` (plain args) and `ExtensionCallSection` (first arg is a
known section name) remain. `disambiguate_extension_call` now checks for section
name in first arg for all extensions, not just ranged ones.

**ERR_103 updated**
Now allows QuotedString in addition to numeric types. String args pass IRDb
validation; extensions reject invalid types at runtime.

**ERR_101, ERR_102 retired**
Error codes for ranged-extension-specific constraints removed along with the
ranged extension concept.

**Engine execute_extensions updated**
Builds `Vec<ExtArg>` instead of `Vec<u64>`. Uses a block scope to isolate the
immutable `&mmap[..]` borrow held by `ExtArg::Section` before the mutable mmap
patch write.

**std extensions updated**
`std::crc32c`, `std::sha256`, `std::md5` all switch to `FirmionExtension` and
receive image data from `args[0]` as `ExtArg::Section`.

Changes:

- `firmion_extension/lib.rs`: Added `ExtArg`, rewrote `FirmionExtension` trait,
  removed `FirmionRangedExtension`.
- `ext/ext.rs`: Removed `RegisteredExtension` enum; `ExtensionEntry.extension`
  is now `Box<dyn FirmionExtension>` directly. Removed `register_ranged`,
  `is_ranged`. Re-exports `ExtArg`.
- `ext/test_mocks.rs`: All 7 mocks updated to new signature; ranged mocks
  receive image via `ExtArg::Section`.
- `std/crc32c`, `std/sha256`, `std/md5`: Converted to `FirmionExtension`.
- `irdb/irdb.rs`: Removed `ExtensionCallRanged` arm; updated disambiguation
  and ERR_103 validation.
- `ir/ir.rs`: Removed `ExtensionCallRanged` variant.
- `engine/engine.rs`: Removed `RegisteredExtension` dispatch; builds `ExtArg`
  list per call.
- `tests/integration.rs`: Removed 4 obsolete tests; updated 3 tests to reflect
  runtime-vs-compiletime error shift. 294 tests pass.

---

## 2026-04-14 -- WrExt eliminated (Step 5)

**IRKind::WrExt removed**
The two-node `ExtensionCall + WrExt` structure collapsed into a single
`ExtensionCall` node, mirroring how `Wrf` works.

Key changes:

- `IRKind::WrExt` removed from `ir/ir.rs`.  `ExtensionCall` and
  `ExtensionCallSection` are now write statements — they advance the location
  cursor and pre-pad zeroed bytes directly.
- `layoutdb/layoutdb.rs`: `wr <extension>` no longer creates a `WrExt` wrapper.
  The `ExtensionCall` LinIR produced by `record_expr_r` is the statement.
- `engine/engine.rs`:
  - `iterate_wrext` renamed to `iterate_ext`; no longer crawls backward via
    `is_output_of()`.  `ExtensionCall | ExtensionCallSection` now handled in the
    iterate match arm (previously in the no-op list).
  - `execute_core_operations`: `WrExt` arm replaced by
    `ExtensionCall | ExtensionCallSection` arm.
  - `execute_extensions`: `operand_consumer: HashMap` eliminated.  Patch offset
    now read from `ir_locs[idx]` (the extension call's own location) instead of
    `ir_locs[consumer_idx]` (the WrExt's location).
- `irdb/irdb.rs`: `WrExt` validation arm removed.

**Output operand retained**
The trailing output operand on `ExtensionCall` and `ExtensionCallSection` is
kept for type-checking: if the extension result is used in arithmetic, `wr8..64`,
`wrs`, or `const`, `DataType::Extension` propagates and IRDb rejects it.  In the
valid `wr <extension>` case the output operand is orphaned (not consumed by any
IR) but harmless.

294 tests pass.

---

## 2026-04-17 — max-output-size flag (region plan step 1)

**Fuzz bug fix**
`pad_addr_offset 0xFFFFFFFFFFFFF; wrs "123";` caused a hang in `execute_wrx`
(petabyte-range pad count).  Fix: pre-execute size check in `process.rs` using
`engine.wr_dispatches.last().map_or(0, |d| d.file_offset + d.size)`.

**New CLI flag**
`--max-output-size SIZE` (default 256M, accepts K/M/G suffix).  Parsed by
`parse_size()` in `src/main.rs`.  Passed as new `max_output_size: u64` parameter
to `process()`.  Rejects before `execute()` with ERR_224.

**New error code**
ERR_224: output image size exceeds `--max-output-size` limit.

**Fuzz target**
`process/fuzz/fuzz_targets/fuzz_target_1.rs` updated to pass `max_output_size`
(64 KiB for fast fuzzing).

**Regression test**
`tests/fuzz_found_19.firm` added to integration suite; expects ERR_224.
`max_output_size_flag` inline test: `--max-output-size 0` on 1-byte output.

---

## 2026-04-17 — fuzz fixes and to_bool() hardening

**fuzz_found_20: wrf extra args (ERR_37)**
`wrf(f), "n"` caused IRDb assert.  Parser now rejects extra args after the
first wrf argument with ERR_37 and advances past `;`.

**fuzz_found_21: wr32 extra args (ERR_111)**
`wr32 a, b, (12)` caused IRDb assert.  `validate_numeric_1_or_2` replaced
`assert!` with graceful ERR_111 diagnostic.  Covers all Wr(n), Align,
SetAddr*, SetSecOffset, SetFileOffset.

**fuzz_found_22: if with string condition (ERR_112)**
`const MODE = ""; if MODE { }` panicked in `to_bool()`.  Fixed two call
sites in `const_eval.rs` (IfBegin arm and eval_ast_condition) with ERR_112.

**to_bool() Option A hardening**
Changed `ParameterValue::to_bool()` return type from `bool` to `Option<bool>`.
All five call sites updated:

- `engine/engine.rs` execute_assert: `expect()` (invariant protected by IRDb)
- `const_eval.rs` IfBegin: `and_then` with ERR_112
- `const_eval.rs` Assert in if/else body: `and_then` with ERR_113
- `const_eval.rs` LogicalAnd/Or: let-else with ERR_114
- `const_eval.rs` eval_ast_condition: match on `to_bool()` with ERR_112

**New error codes**
ERR_113: assert condition in if/else body is non-numeric.
ERR_114: `&&`/`||` operand is non-numeric.

**New test files**
`fuzz_found_20.firm`, `fuzz_found_21.firm`, `fuzz_found_22.firm`,
`const_bool_string_assert.firm`, `const_bool_string_and.firm`,
`const_bool_string_or.firm`.

**New error code registry**
`docs/error-codes.md`: unified list of all error codes by prefix with
next-available summary line.

312 tests pass.

---

## 2026-04-18 -- Recursive depth guards (ERR_38, ERR_39, ERR_115)

**parse_pratt depth guard (ERR_38)**
Added `MAX_PRATT_DEPTH = 200` check at the top of `parse_pratt`.  All recursive
call sites inside `parse_pratt` pass `depth + 1`; top-level entry points pass `0`.

**parse_if_r / parse_if_body_r mutual recursion guard (ERR_39)**
Added `MAX_IF_DEPTH = 100` check at the top of `parse_if_r`.  `parse_if_body_r`
carries the depth parameter and passes `depth + 1` when recursing into nested `if`.

**parse_function_args depth threading (root cause of fuzz SIGSEGV)**
`parse_function_args` was calling `parse_pratt(0, 0, ...)` internally, resetting
the depth counter to zero and bypassing ERR_38.  Fixed by threading `depth` through
`parse_function_args` and passing `depth + 1` at both call sites inside `parse_pratt`
and both internal `parse_pratt` calls within `parse_function_args`.

**eval_const_expr_r depth guard (ERR_115)**
Added `MAX_EVAL_DEPTH = 100` check (matching `Linearizer::MAX_RECURSION_DEPTH`) to
`eval_const_expr_r`.  Adds `depth: usize` parameter threaded through all 7 internal
recursive calls.  Six external call sites pass `0`.  For inputs going through the
linearizer, ERR_203 fires first; ERR_115 provides defense-in-depth for eval paths
that bypass the linearizer.

**Error codes**
ERR_38, ERR_39, ERR_115 added to `docs/error-codes.md`.
Next available: ERR_40, ERR_179, ERR_202, ERR_116, ERR_215, ERR_225, ERR_232.

Regression tests: `fuzz_found_23` (250-level `f(f(...))` fires ERR_38),
`fuzz_found_24` (5000-term flat `1 + 1 + ...` fires ERR_203).

314 tests pass.

---

## 2026-04-21 — Remove output address argument (region plan step 2)

**output statement address removed**
`output <section> <addr>;` syntax removed.  Use `set_addr <addr>;` as the first
statement inside the section instead.  Old syntax emits ERR_50.

**Pipeline cleanup**
Removed: `Output.addr_nid`, `LayoutDb.output_addr_str/loc`,
`IRDb.start_addr`, `Engine.start_addr`, `Engine::new abs_start` param,
`Engine::iterate abs_start` param.  `MapDb.base_addr` now derived from
the first WrDispatch address rather than `engine.start_addr`.

**build_dispatches: leading set_addr skipped for section address**
When a section starts with one or more `set_addr` instructions,
`build_dispatches` now scans past them (to `content_idx`) so the
WrDispatch `addr` field reflects the address after the first `set_addr`
rather than the address at SectionStart (before set_addr runs).
This keeps map output consistent with what `addr()` reads inside the section.

**`__OUTPUT_ADDR` behavior**
`__OUTPUT_ADDR` is now always `addr(<output-section>)`, which equals 0 at
SectionStart before any `set_addr`.  It no longer tracks a separate base set
in the output statement.

**New error code**
ERR_50: `output` address argument; use `set_addr` or region binding.

**Test migration**
~80 `.firm` test files updated.  Zero-address files: remove address.
Non-zero-address files: remove address + add `set_addr <addr>;` as first
statement inside the output section.  Special cases: `wrx_4` expected byte
values recalculated with `addr(foo)=0`; `output_addr_1` and
`output_addr_section_set_addr_with_output_base` repurposed to test
`__OUTPUT_ADDR == 0`; `const_as_output_addr_1` uses `set_addr LOAD_ADDR;`.

320 tests pass.

---

## 2026-04-25 — validation_phase extraction + region plan Steps 3+4 AST layer

**validation_phase crate**
Extracted `execute_validation`, `execute_assert`, `assert_info`, and
`assert_info_operand` from `exec_phase` into a new `validation_phase` crate.
`process.rs` now calls `ValidationPhase::validate` between `LayoutPhase::build`
and `ExecPhase::execute` (error code ERR_225).  Assert evaluation confirmed
independent of generated bytes: `ParmValDb` is immutable during exec and no IR
instruction reads back output-file bytes.

**Region plan Steps 3+4 — AST layer**
Lexer, parser, and AstDb changes for region declarations and section bindings.

New tokens:

- `LexToken::Region`, `LexToken::In` — keywords (added to lexer scan_word).
- `LexToken::RegionProp` — synthetic; children of Region root, val = property
  name ("addr", "size", "default_align", "default_fill"), one expression child.
- `LexToken::RegionRef` — synthetic; second child of Section root when
  `section NAME in REGION` is parsed, val = region name, no children.

New types:

- `RegionEntry` in `ast` crate: holds `nid`, `src_loc`, `addr`, `size`,
  `default_align` (sentinel 1), `default_fill` (sentinel 0xFF).
  `addr`/`size` filled by const_eval (Step 3 const_eval, not yet done).

Parser changes:

- `parse_region` / `parse_region_contents`: parse `region NAME { prop = expr; }`.
- `parse_section`: optional `in REGION` binding after section name.

AstDb changes:

- `AstDb::regions: HashMap<String, RegionEntry>` added.
- `record_region`: reserved name (ERR_56), duplicate name (ERR_55).
- `AstDb::new`: Region nodes processed at root; region/section/const name
  conflicts detected (ERR_43, ERR_58); section region-ref validation (ERR_51,
  ERR_52).
- `Section::region: Option<String>` added; set from RegionRef child if present.

Linearizer: `RegionRef` added to syntactic noise (no IR emitted).

New error codes: ERR_40–49, ERR_51–64, ERR_225.

**Region integration tests**
15 new test fixtures and functions covering all region error codes (ERR_40–64)
plus one success case (`region_valid`).

**Bug fix: infinite loop in ERR_40/ERR_41 paths**
`parse_region_contents` error paths for unknown and duplicate property names
called `advance_past_semicolon()` while the property name token was still the
current (not yet consumed) token.  `advance_past_semicolon` checked the
previous token, saw a `;` from the preceding valid property, and did nothing,
looping forever.  Fix: `self.tv.skip()` before `advance_past_semicolon()` in
both paths.

334 tests pass.

---

## 2026-04-26 — Region plan Step 3 evaluation (address anchor)

**const_eval::evaluate_regions**
New public function in `const_eval/const_eval.rs`.  Called in `process.rs` after
`AstDb::new(true)` and before `LayoutDb::new`.  Walks each RegionProp child of
each Region node, lowers the expression subtree with `Linearizer::record_expr_r`,
evaluates with `ConstIR::eval_const_expr_r`, and stores `addr`, `size`,
`default_align`, `default_fill` into the matching `RegionEntry`.  Emits ERR_180
for non-numeric property values.

**Section address anchoring in LayoutPhase**
`LayoutPhase` gains `section_anchors: HashMap<String, u64>` (section name to
region addr).  `build()` accepts this map as a new parameter.  `process.rs`
builds it from the resolved `pruned_ast_db` after `evaluate_regions` and passes
it to `LayoutPhase::build`.

`iterate_section_start` applies the anchor for region-bound sections:
sets `current.addr.addr_base = anchor` and `current.addr.addr_offset = 0`.
`ir_locs[lid]` is re-recorded after the anchor is applied so that
`addr(section_name)` returns the region-anchored address, not the pre-entry
parent address.

**New error codes**
ERR_180: region property value is not numeric.
ERR_226: evaluate_regions failure halt.

**Tests**
`region_anchor.firm`: writes `addr(top)` as `wr32`; verifies bytes equal
`0x08000000` LE, confirming the anchor is applied correctly.
`region_exec66.firm`: string-valued region property triggers ERR_180.

336 tests pass.

---

## 2026-04-26 — Region plan Steps 4-5 (RegionProps, size enforcement, ERR_183)

**RegionProps on IRDb**
Removed `section_anchors: HashMap<String, u64>` from `LayoutPhase`.  Added
`RegionProps { addr: u64, size: u64 }` to `ir/ir.rs` and
`section_regions: HashMap<String, RegionProps>` to `IRDb`.  Built in
`process.rs` from `pruned_ast_db.sections` and `pruned_ast_db.regions`.
All downstream phases read region data from `irdb.section_regions`.

**default_align and default_fill removed**
These region properties were unimplemented and had no planned use in the
near term.  Removed from `RegionEntry`, `parse_region_contents`, and
`evaluate_regions`.  ERR_40 message updated to "expected addr or size".

**Region size enforcement (ERR_185, ERR_186, ERR_187)**
`iterate_set_addr` in `layout_phase.rs` now accepts `irdb`, `lid`, and
`diags`.  For region-bound sections:

- ERR_185: `set_addr` target is outside `[binding.addr, binding.addr+binding.size)`.
- ERR_187: `binding.addr + binding.size` overflows u64 (uses `checked_add`).

`validate_section_regions` called after `iterate` converges:

- ERR_186: `sizeof(section)` exceeds `binding.size`.

Deduplication via `warned_lids: HashSet<(usize, &'static str)>` prevents
repeated errors across iterate passes.

**Overlapping region detection (ERR_183)**
`evaluate_regions` now checks all region pairs after property evaluation.
Two regions with non-zero size that share any address range emit ERR_183.
`region_nested_2.firm` converted from a success fixture to an ERR_183
error fixture.

**New error codes**
ERR_183: overlapping regions.
ERR_185: set_addr target outside region bounds.
ERR_186: section size exceeds region size.
ERR_187: region addr + size overflows u64.

342 tests pass.

---

## 2026-04-27 — K/M/G magnitude suffix for integer literals

Decimal integer literals now accept an optional K/M/G magnitude suffix before
the type suffix: `64K` = 65536, `1M` = 1048576, `2Gu` = U64(2147483648),
`-1Ki` = I64(-1024).

Full token forms: `<digits>[K|M|G][u|i]?` (decimal only; hex/binary unchanged).

Changes:

- `ast/lexer.rs`: `consume_decimal_suffix` checks for K/M/G before u/i.
  `scan_negative_i64` checks for K/M/G before the optional `i` suffix.
- `ir/ir.rs`: `strip_kmg(&str) -> (&str, u64)` added as a public free function.
  All three numeric arms of `IROperand::convert_type` (U64, I64, Integer) call
  `strip_kmg` after stripping the type suffix and apply `checked_mul` for
  overflow detection.
- `const_eval/const_eval.rs`: imports `strip_kmg` from `ir`. All three integer
  arms of the inline literal evaluator (Integer/ERR_88, U64/ERR_89, I64/ERR_90)
  apply `strip_kmg` + `checked_mul`. Overflow is reported via the existing
  ERR_88/23/24 codes.
- 5 new integration tests: `kmg_suffix_1`, `kmg_suffix_2`, `kmg_suffix_3`,
  `kmg_overflow_u64`, `kmg_overflow_i64`.
- `README.md`: region size example updated to use `64K` and `1M`.

347 tests pass.

---

## 2026-04-28 — addr(REGION) and sizeof(REGION)

`addr()` and `sizeof()` now accept region names in addition to section names.

**Design**
Same `IRKind::Addr` / `IRKind::Sizeof` and same identifier operand structure as
sections.  Regions provide their values from `irdb.region_bindings` (const-
evaluated, stable across layout iterations) rather than from `ir_locs[]`.

Changes:

- `layoutdb/layoutdb.rs`: `LayoutDb` gains `region_names: HashSet<String>`,
  populated from `ast_db.regions` in `new()`.  `IdentDb::verify_operand_refs`
  checks `lindb.region_names` before emitting ERR_208, so region names pass
  the identifier validation gate.
- `irdb/irdb.rs`: `IRDb` gains `region_bindings: HashMap<String, RegionProps>`
  (keyed by region name, parallel to `section_regions` which is keyed by section
  name).  `IRDb::new` accepts and stores this map.
- `process/process.rs`: passes `region_bindings` (returned by `evaluate_regions`,
  not consumed when building `section_regions`) to `IRDb::new`.
- `layout_phase/layout_phase.rs`:
  - `iterate_sizeof`: falls back to `irdb.region_bindings` when the name is not
    in `sized_locs`; returns `binding.size`.  ERR_129 message updated to mention
    regions.  ERR_169 message updated to "section or region names".
  - `iterate_identifier_address`: restructured into three paths — section/label,
    region, not-found.  For regions: `addr(REGION)` returns `binding.addr`;
    `addr_offset`/`sec_offset`/`file_offset` applied to a region emit ERR_189.
    ERR_135 message updated to "section, label, or region".
- New error code ERR_189: offset-style address operation applied to a region name.
- `tests/region_addr_sizeof.firm`: verifies `addr()` and `sizeof()` on two
  regions with asserts and `wr32` output bytes.
- 1 new integration test: `region_addr_sizeof`.

348 tests pass.

---

## 2026-04-27 — RegionProps diagnostic fields (name + src_loc)

`RegionProps` gained two new fields — `name: String` and `src_loc: SourceSpan`
— so every region error site can name the region and point at its declaration
without extra lookups.  `Copy` dropped (String prevents it); `Clone` retained.

Changes:

- `ir/ir.rs`: `RegionProps` now carries `name` and `src_loc`.  `Copy` derive removed.
- `const_eval/const_eval.rs`: binding constructor populates `name` and `src_loc`.
  ERR_188 message includes region name, points at declaration site.
  ERR_183 upgraded from `err1` to `err2`; both overlapping declarations highlighted.
  Redundant `name_a`/`name_b` destructures in the overlap loop replaced with `_`.
- `process/process.rs`: `*binding` (copy) changed to `binding.clone()`.
- `layout_phase/layout_phase.rs`:
  - ERR_185 upgraded to `err2`; message includes region name; second label at declaration.
  - ERR_186 upgraded to `err2`; message includes region name; second label at declaration.
  - ERR_187 upgraded to `err2`; message includes region name; second label at declaration.

347 tests pass.

---

## 2026-04-28 — Nested region support (region_intersection + ERR_190)

**ERR_183 relaxed for containment**
`evaluate_regions` in `const_eval.rs` previously rejected any two regions that
shared any address range, including containment (A fully inside B).  The check
now detects only *partial* overlap (ranges overlap but neither contains the other).
Containment is allowed so that inner sections can bind to sub-regions of an outer
region without triggering ERR_183.

**region_intersection on ScopeFrame**
`layout_phase.rs` gains `region_intersection: Option<RegionProps>` on `ScopeFrame`.
At each `SectionStart`, the effective constraint is computed as the intersection
of the parent frame's `region_intersection` and the current section's direct
region binding (from `irdb.region_for_section`):

- No constraints: `None` (no enforcement).
- Parent only or direct only: inherit as-is.
- Both: compute geometric intersection -- `addr = max`, `end = min(ends)`.
  - Non-empty: `Some(intersection)` with name `"{parent} & {direct}"`.
  - Empty (disjoint): emit ERR_190, fall back to direct binding.

`iterate_set_addr` now reads `frame.region_intersection` instead of calling
`irdb.region_for_section`.  This means inner sections without a direct binding
still get ERR_185 enforcement from the inherited parent region.

`intersect_regions` static helper added to `LayoutPhase`.

**New error code ERR_190**
ERR_190: section's direct region binding does not intersect the enclosing
region inherited from the parent scope (mutually exclusive regions in nested
sections).  Emitted in `iterate_section_start` via `warned_lids` deduplication.

Tests:

- `region_exec70_partial.firm`: two partially-overlapping regions (A straddles B)
  still trigger ERR_183. Integration test `region_exec70_partial`.
- `region_nested_2.firm`: comment updated; test changed from failure (ERR_183) to
  success -- FLASH1 fully inside FLASH is valid containment.
- `region_nested_containment.firm`: outer in FLASH, inner (called via `wr`) in CODE
  where CODE is a subset of FLASH; both fit; no errors. Integration test
  `region_nested_containment`.
- `region_nested_exec72.firm`: inner section (no direct binding) inherits FLASH
  from outer; `set_addr` outside FLASH triggers ERR_185. Integration test
  `region_nested_exec72`.
- `region_nested_exec77.firm`: inner bound to SRAM (disjoint from outer's FLASH);
  ERR_190 fires. Integration test `region_nested_exec77`.

352 tests pass.

---

## 2026-04-28 — Allow partial region overlap; intersection-based ERR_186

**ERR_183 retired**
The compile-time region overlap check (ERR_183) is removed entirely.  Any two
regions may share address space -- containment, partial overlap, or identical
ranges are all valid.  Enforcement that writes stay within the effective
intersection is handled at layout time by the `section_effective_regions` map
and `validate_section_regions`.

**section_effective_regions on LayoutPhase**
`LayoutPhase` gains `section_effective_regions: HashMap<String, RegionProps>`.
`iterate_section_start` writes the converged `region_intersection` into this map
on every iterate pass; the final pass holds the post-convergence value.

`validate_section_regions` now prefers `section_effective_regions[sec]` over the
raw `irdb.region_for_section()` lookup.  When two regions partially overlap, the
stored intersection is tighter than the direct binding, so ERR_186 fires if the
section's byte count exceeds the intersection size rather than the (wider) direct
binding size.

Tests:

- `region_exec70_partial.firm` repurposed: outer in A, inner in B (partial
  overlap A & B = 128 bytes); inner writes exactly 128 bytes -- succeeds.
  Integration test `region_exec70_partial` changed from failure to success.
- `region_exec73_partial_overlap.firm`: same regions, inner writes 192 bytes;
  exceeds intersection 128 -> ERR_186.  Integration test `region_exec73_partial_overlap`.

353 tests pass.

---

## 2026-04-28 — EffectiveRegion backtrace + ERR_191 starting-address check

**EffectiveRegion struct**
Replaced the ad-hoc `region_intersection: Option<RegionProps>` + separate
`region_contributors: Vec<RegionProps>` fields on `ScopeFrame` with a single
`effective_region: Option<EffectiveRegion>` field.  `EffectiveRegion` carries:

- `binding: RegionProps` — the geometric intersection (used for ERR_185/73/74
  enforcement, unchanged).
- `contributors: Vec<RegionProps>` — every region that narrowed the bound,
  outermost first, for use in ERR_186 backtrace diagnostics.

`section_effective_regions: HashMap<String, EffectiveRegion>` (was
`HashMap<String, RegionProps>`) persists the converged value for
`validate_section_regions`.

**ERR_186 backtrace via err_with_locs**
`diags::Diags` gains `err_with_locs(code, msg, primary, &[(SourceSpan, String)])`:
builds an ariadne report with one red primary label and N yellow secondary labels,
each carrying its own `.with_message(text)`.  When ERR_186 fires and
`contributors.len() > 0`, one yellow label per contributing region is emitted,
each annotated with `"region 'X': addr=0x…, size=…"`.  The addr/size text is
necessary because ariadne shows only the `region NAME {` declaration line, not
the property values.

**Anchor fix (README consistency)**
`iterate_section_start` previously anchored `addr_base` to `intersection.addr`
(the effective intersection start).  The README states that a section `in R`
always starts at `R.addr`.  Anchor changed to `d.addr` (direct region's addr).

**ERR_191 — starting address outside parent intersection**
When a section has both a parent inherited region and a direct binding, and the
intersection is non-empty but `d.addr < intersection.addr` (the direct region
starts before the parent region), ERR_191 fires.  This covers the case where
the anchor `d.addr` would lie outside the effective parent constraint.  Emitted
via `err2` with both region declaration sites highlighted.

New error code: ERR_191.

Tests:

- `region_exec78_bad_start.firm`: A=[0x1080,0x100), B=[0x1000,0x200); B starts
  before A; intersection non-empty but B.addr (0x1000) < intersection start
  (0x1080); ERR_191 fires.  Integration test `region_exec78_bad_start`.

354 tests pass.

---

## 2026-04-28 — RegionDb crate; ERR_192 region-bound section re-use

**RegionDb crate**
New first-class pipeline stage inserted between IRDb and LayoutPhase.

`regiondb/regiondb.rs` walks `irdb.ir_vec` once, tracking section nesting via
a scope stack, and computes `EffectiveRegion` for every section.  Because
region `addr` and `size` are const-evaluated before IRDb exists, the resulting
intersections are stable — they do not change across layout passes.

`EffectiveRegion` moved from `layout_phase.rs` to `ir/ir.rs` so both `regiondb`
and `layout_phase` can share the type without a circular dependency.

`intersect_regions` helper moved from `LayoutPhase` (a private static method
recomputed on every iterate pass) to `regiondb/regiondb.rs` (a module-level
free function called exactly once during `RegionDb::build`).

ERR_190 and ERR_191 detection moved from `layout_phase::iterate_section_start`
(fired with `warned_lids` deduplication across every iterate pass) into
`RegionDb::build` (fired once, naturally deduplicated).

LayoutPhase simplification:

- `EffectiveRegion` struct definition removed (now in `ir`).
- `intersect_regions` static method removed (now in `regiondb`).
- `section_effective_regions: HashMap<String, EffectiveRegion>` field removed.
- `iterate_section_start`: reduced to a RegionDb lookup + scope push + anchor.
  No intersection computation, no error emission, no `lid`/`diags` parameters.
  Returns `()` instead of `bool`.
- `validate_section_regions`: reads from `RegionDb` instead of the removed map.
- `build` and `iterate` gain `region_db: &RegionDb` parameter.

ERR_192 — region-bound section used more than once: Region-bound sections
anchor to a fixed address on every inclusion; a second `wr` is guaranteed to
produce an address conflict.  `RegionDb::build` detects the second `SectionStart`
for any region-bound section and emits ERR_192.

Pipeline ordering: `process.rs` builds `RegionDb` immediately after `IRDb`,
before `LayoutPhase`.  Failure halts with ERR_227.

New error codes: ERR_192, ERR_227.

Tests:

- `region_exec79_reuse.firm`: `wr pinned` twice where `pinned in FLASH`;
  ERR_192 fires.  Integration test `region_exec79_reuse`.

355 tests pass.

---

## 2026-05-02 — Rename pad_* offset commands

**Syntax rename**
Three layout-time padding commands renamed for clarity:

- `set_addr_offset` → `pad_addr_offset`
- `set_sec_offset`  → `pad_sec_offset`
- `set_file_offset` → `pad_file_offset`

The `pad_` prefix makes intent explicit: these commands pad forward to a target
offset; they do not set an absolute address.

Changes:

- `ast/lexer.rs`: keyword table entries renamed.
- `ir/ir.rs`: `IRKind` variants renamed (`PadAddrOffset`, `PadSecOffset`, `PadFileOffset`).
- `layout_phase/layout_phase.rs`: match arms updated to new variant names.
- `tests/firmion_fuzz.dict`: new names added; old names removed.
- All test fixtures in `tests/*.firm` updated.
- `process/fuzz/seeds/*.firm` and `process/fuzz/seeds/firmion_fuzz.dict` updated.
- `docs/`: `02-system.yaml`, `region-plan.md`, and this file updated.
- `vscode-firmion/CHANGELOG.md`: keyword list updated.

355 tests pass.

---

## 2026-05-03 -- obj declaration and wr obj redesign

**Design**
Replaced the point-solution `wrobj "section", "file"` command with a holistic
`obj` declaration that binds a name to a section in an external object file.
The name is then used with `wr` and `sizeof` exactly like a section name.

```firmion
obj rodata = ".rodata" in "tests/obj_test.elf";
section firmware {
    wr rodata;
    assert sizeof(rodata) == 23;
}
output firmware;
```

Key design decisions:

- `obj` is a top-level declaration (parallel to `region`).
- `wr obj_name` dispatches to `IRKind::Wrobj` at layoutdb time (like `wrf`).
- `sizeof(obj_name)` resolves via `irdb.obj_sections` in `iterate_sizeof`.
- Section/const/region name conflict checks added (ERR_64, ERR_65, ERR_66).
- `obj_sections` keyed by declared name (was `(section_name, file_path)` tuple).
- `wrobj` keyword and `LexToken::Wrobj` removed; `LexToken::Obj` added.

Changes:

- `ast/lexer.rs`: removed `wrobj` keyword, added `obj`.
- `ast/ast.rs`: added `parse_obj`, `LexToken::Obj` top-level dispatch.
- `astdb/astdb.rs`: `ObjDecl` struct, `obj_decls` map, `record_obj`,
  conflict checks (ERR_62..71), `validate_nesting_r` obj bypass.
- `layoutdb/layoutdb.rs`: `obj_decls`/`obj_names` fields, `wr obj_name`
  path emits `IRKind::Wrobj`, removed `LexToken::Wrobj` arm,
  `verify_operand_refs` accepts obj names.
- `linearizer/linearizer.rs`: removed `LexToken::Wrobj` from `tok_to_irkind`.
- `irdb/irdb.rs`: `obj_decls` field, `obj_sections` rekeyed by declared name,
  `validate_wrobj_operands` rewritten for single Name operand,
  `resolve_obj_section` takes declared name.
- `layout_phase/layout_phase.rs`: `iterate_wrobj` uses single Name operand,
  `iterate_sizeof` gains obj path.
- `exec_phase/exec_phase.rs`: `execute_wrobj` uses single Name operand.
- `docs/error_codes.md`: ERR_62..71 added; next available updated to ERR_67.
- Test fixtures `wrobj_*.firm` rewritten for new syntax.
- `tests/integration.rs`: `wrobj_wrong_args` now checks ERR_61.

361 tests pass.

---

## 2026-05-04 — obj brace-block syntax + obj_align/obj_lma/obj_vma

**Two changes shipped in one session.**

### obj brace-block syntax

Converted `obj` declaration from inline `= "section" in "file"` style to
brace-block style: `obj name { section = "..."; file = "..."; }`.  Properties
may appear in any order.  The synthesized `ObjProp` AST node carries the
property name in `val` (matched by string, not by token type, to avoid the
`LexToken::Section` collision).  Error codes ERR_61..77 were assigned and
individualized; ERR_71 fires when a required property is missing.  The
`error_codes_are_unique` test was extended to scan `err_expected_after` and
`err_invalid_expression` wrapper calls.  `obj_names: HashSet` removed from
`LayoutDb`; replaced by `obj_decls.contains_key`.

Changed files: `ast/lexer.rs`, `ast/ast.rs`, `astdb/astdb.rs`,
`layoutdb/layoutdb.rs`, `tests/wrobj_*.firm`, `tests/integration.rs`,
`docs/error_codes.md` (ERR_62..77, next ERR_73).

### obj_align / obj_lma / obj_vma

Three new layout-time expression builtins returning U64.  All follow the
`sizeof(name)` pattern: one mandatory identifier in parens, one output operand.

- `obj_align(obj)` -- section alignment (unified `object` API).
- `obj_vma(obj)` -- section virtual address (unified `object` API).
- `obj_lma(obj)` -- section load address (ELF program-header scan).
  Non-ELF recognized formats: ERR_122.  Unrecognized formats: ERR_120.
  With current features `["read","elf"]`, only ELF is parseable; ERR_122
  is reserved for future COFF/Mach-O feature additions.

`ObjSectionInfo` gained `align`, `vma`, `lma` fields.  `parsed_obj_files`
cache updated to `HashMap<String, SectionProps>` (adds align/vma/lma).
`compute_lma_from_segments` free function scans PT_LOAD program headers via
`object::File::Elf32/Elf64` match + `ProgramHeader` trait.

Changed files: `ir/ir.rs`, `ast/lexer.rs`, `ast/ast.rs`,
`linearizer/linearizer.rs`, `irdb/irdb.rs`, `layout_phase/layout_phase.rs`,
`exec_phase/exec_phase.rs`, `const_eval/const_eval.rs`,
`docs/error_codes.md` (ERR_73, ERR_122, next ERR_74/ERR_123),
`docs/ai/02-system.yaml`, `tests/obj_align_1.firm`, `tests/obj_vma_1.firm`,
`tests/obj_lma_1.firm`, `tests/obj_lma_non_elf.firm`, `tests/integration.rs`.

365 tests pass.

---

## 2026-05-06 — wrbe8..wrbe64 big-endian write instructions

Full set of big-endian integer write instructions, parallel to wr8..wr64.

Design: `IRKind::Wr(u8)` extended to `IRKind::Wr(u8, bool)` where the bool
is `true` for big-endian. All existing `Wr(w)` match sites updated to `Wr(w, _)`;
the compiler enforced exhaustiveness at every site. Only `exec_phase` inspects
the flag.

Changes:

- `ir/ir.rs`: `IRKind::Wr(u8, bool)`.
- `ast/ast.rs`: `LexToken::Wrbe8`..`Wrbe64` variants; `is_section_expr_tok`
  extended; `is_reserved_identifier` gains `"wrbe" + digit` prefix check.
- `ast/lexer.rs`: `"wrbe8"`..`"wrbe64"` keyword entries.
- `layoutdb/layoutdb.rs`: `Wrbe8`..`Wrbe64` added to dispatch arm; direct
  `IRKind::Wr(1)` fill call updated to `IRKind::Wr(1, false)`.
- `linearizer/linearizer.rs`: LE tokens updated to `Wr(n, false)`; BE tokens
  mapped to `Wr(n, true)`.
- `irdb/irdb.rs`: `Wr(_, _)` pattern.
- `layout_phase/layout_phase.rs`: `Wr(w, _)` in size helper and iterate dispatch.
- `exec_phase/exec_phase.rs`: `Wr(w, _)` in width helper and dispatch;
  `execute_wrx` branches on the flag: `to_be_bytes()` with trailing `byte_size`
  slice (`buf[8-byte_size..8]`) vs `to_le_bytes()` with leading slice.
- `tests/firmion_fuzz.dict`: all 8 wrbe keywords added.
- `vscode-firmion/syntaxes/firmion.tmLanguage.json`: wrbe variants added to regex.
- `docs/ai/02-system.yaml`: `wrbe8/wrbe16/.../wrbe64` documented.
- `tests/wrx_7.firm` + `tests::wrx_7` integration test: all 8 widths verified
  against known big-endian byte sequences.

367 tests pass.

---

## 2026-05-06 — std::xor extension

New `std::xor` standard extension.  Computes an XOR checksum over a
caller-specified image region and writes the 1-byte result into the output image.

Follows the same crate layout, feature-gating, and `FirmionExtension` trait
implementation as `std::crc32c`, `std::sha256`, and `std::md5`.

Changes:

- `std/xor/src/xor.rs`: `Xor` struct implementing `FirmionExtension`.
  `size()` = 1.  `execute` folds all bytes with `^`.
- `std/xor/Cargo.toml`: new crate `std_xor`, no external dependencies.
- `std/xor/tests/integration.rs`: 4 tests --
  `xor_section_form`, `xor_explicit_range`, `xor_sizeof`, `xor_ambiguous_section`.
- `std/xor/tests/*.firm`: 4 test fixtures.
- `extensions/Cargo.toml`: `std-xor` feature + `std_xor` optional dep.
- `extensions/src/lib.rs`: `#[cfg(feature = "std-xor")] std_xor::register(...)`.
- `process/Cargo.toml`: `std-xor` feature propagated to extensions.
- `Cargo.toml`: `std/xor` workspace member; `std-xor` default feature.
- `docs/ai/03-structure.yaml`: `std/xor` added to `std_extensions`.

All 4 std::xor tests pass.

---

## 2026-05-07 — String const tests

No functional changes. Added tests confirming that string consts already work
end-to-end across the full pipeline.

New test fixtures and integration tests:

- `tests/const_string_wrf_1.firm` + `const_string_wrf_1`: source literal
  `const F = "..."; wrf F;` reads the file and verifies output bytes.
- `tests/const_string_wrf_2.firm` + `const_string_wrf_2`: CLI define
  `-DF="..."` supplies the path; `const F;` in source declares without default.
- `tests/const_string_if_1.firm` + `const_string_if_1`: string const in an
  `if` guard prunes the dead else branch; verifies `[0x01]` in output.
- `tests/const_string_if_2.firm` + `const_string_if_2`: same but the variant
  name comes from `-DVARIANT="v1"` CLI define.

`docs/ai/02-system.yaml`: `const` entry expanded to document string const
support and the CLI quoting requirement.

371 tests pass.

---

## 2026-05-08 — trace instruction (IRKind::Trace)

New `trace` statement for verbose layout diagnostics, controlled by the `-v` flag.

Execution model: fires every layout iteration in `LayoutPhase::iterate` (gated on `diags.trace_enabled()`),
once in `ValidationPhase::validate` for pre-output traces, and once in `ExecPhase::execute_post_output`
for post-output traces.  Each output line is prefixed `[Trace-N]` where N is the 1-based layout iteration
count.  Silent when `-v` is not supplied.

`Diags::trace_enabled()` returns `self.verbosity >= 2`.  Default verbosity is 1 (no flags); `-v` raises
it to 2.  `pub trace_iteration: usize` added to `Diags`; `LayoutPhase::iterate` writes the 1-based count
at the start of each pass.  After layout converges the final count is read unchanged by
`ValidationPhase` and `ExecPhase`.

Language surface: same string-expression operand form as `print`; comma-separated mix of quoted strings
and expressions.  Example: `trace "size of A = ", sizeof(A), "\n";`

Changes:

- `diags/diags.rs`: `trace_enabled() -> bool` (`verbosity >= 2`); `pub trace_iteration: usize`.
- `ir/ir.rs`: `IRKind::Trace` variant.
- `ast/ast.rs`: `LexToken::Trace`; `is_section_expr_tok` + `is_reserved_identifier` + parse dispatch.
- `ast/lexer.rs`: `"trace"` keyword entry.
- `astdb/astdb.rs`: `Trace` routed to pre/post output globals (same as `Print`).
- `layoutdb/layoutdb.rs`: `LexToken::Trace` in write/print dispatch arm.
- `linearizer/linearizer.rs`: `LexToken::Trace => IRKind::Trace`.
- `irdb/irdb.rs`: `IRKind::Trace` alongside `Print` in `validate_string_expr_operands`.
- `layout_phase/layout_phase.rs`: `diags.trace_iteration = iter_count`; `IRKind::Trace if diags.trace_enabled()` arm fires with prefix.
- `validation_phase/validation_phase.rs`: `IRKind::Trace if diags.trace_enabled()` arm (pre-output only).
- `exec_phase/exec_phase.rs`: `IRKind::Trace` no-op in `execute_core_operations`; fires in `execute_post_output`.
- `tests/trace_1.firm`, `tests/trace_pre_post_1.firm`: fixtures.
- `tests/integration.rs`: `trace_silent_1`, `trace_pre_output_1`, `trace_post_output_1`, `trace_order_1`.

388 tests pass.

---

## 2026-05-21 -- flexiglob integration: glob patterns for obj file/section

**flexiglob dependency**
Added `flexiglob = { path = "../flexiglob" }` to workspace `[workspace.dependencies]`
and `flexiglob = { workspace = true }` to `irdb/Cargo.toml`.

**ObjProps extended**
`ir/ir.rs` `ObjProps` gained two new fields: `file_exclude: String` and
`section_exclude: String`.  `const_eval::evaluate_obj_props` populates them via
`resolve_optional_obj_prop` (returns empty string when absent).  Both accept
flexiglob patterns or literal strings.

**irdb/objfile.rs rewritten**
`ObjFileResolver` now accepts flexiglob patterns for all four fields.
Key internals:

- `SectionCandidate` -- local struct used as the `T` type parameter for `GlobOperator<T>`.
- `SortByAlignmentOp` -- SORT_BY_ALIGNMENT operator; stable descending sort by `align`.
- `SortByInitPriorityOp` -- SORT_BY_INIT_PRIORITY operator; sorts by trailing numeric suffix
  (e.g. `.init_array.00100` -> 100; no suffix sorts last).
- `enumerate_file_candidates(hint)` -- filesystem traversal driven by `scan_hint()`.
- `ObjFileResolver::resolve()` returns `Option<Vec<ObjsecInfo>>` with three steps:
  1. File glob + file_exclude; empty -> ERR_236.
  2. Section glob with operators + section_exclude; empty -> ERR_119.
  3. LMA continuity check -> ERR_235.

**IRDb.objsecs type change**
`objsecs: HashMap<String, ObjsecInfo>` changed to
`objsecs: HashMap<String, Vec<ObjsecInfo>>`.
All consumers updated: `validate_wrobj_operands`, `validate_obj_query_operands`,
`iterate_wrobj`, `iterate_sizeof`, `iterate_obj_align/vma/lma`, `execute_wrobj`.
obj_align/vma/lma queries use `infos[0]` (first matched section).

**layout_phase::packed_size**
New `pub fn packed_size(infos: &[ObjsecInfo]) -> u64` in `layout_phase.rs`.
Computes total byte footprint with inter-section alignment padding, mirroring the
byte sequence written by `execute_wrobj`.  Used by both layout_phase and exec_phase.

**exec_phase::execute_wrobj**
Rewrites from single-file to multi-section iteration.  Writes zero-fill alignment
padding between sections; tracks cumulative `write_offset` for pad computation.

**New error codes**
ERR_233: glob pattern syntax error.
ERR_234: unknown glob operator name.
ERR_235: matched sections are not LMA-contiguous.
ERR_236: file glob pattern matched no files (replaces ERR_118 for the "file not found"
case -- a literal path that does not exist now emits ERR_236 via the "no matches"
branch rather than an I/O error branch).

**Tests**
New ELF fixture: `tests/glob_test.elf` (ELF32 ARM, .text=4B align=4 VMA=0x1000,
.data=8B align=8 VMA=0x2000, separate non-contiguous segments).
6 new test fixtures + 6 integration tests covering:

- literal section match + write bytes (`wrobj_glob_sec`)
- ERR_235 multi-section LMA discontinuity (`wrobj_glob_err235`)
- ERR_119 via section_exclude removing all matches (`wrobj_glob_sec_excl`)
- ERR_236 via file_exclude removing all matches (`wrobj_glob_file_excl`)
- ERR_233 bad glob syntax (`wrobj_glob_bad_syntax`)
- ERR_234 unknown operator (`wrobj_glob_unknown_op`)

`wrobj_bad_file` integration test updated: expects ERR_236 (was ERR_118).

397 tests pass.

---

## 2026-05-05 — Typed-lowering architecture integration

**Const Evaluation as AST Walker**
`const_eval` transitioned into a pure AST walker, evaluating and pruning in one pass. It clones the input AST into a "working" pruned AST. `LexToken::Const` evaluation correctly differentiates between declarations (`const X;`) and assignments (`const X = 1;`) by inspecting the node structure dynamically.

**Typed Lowering in Linearizer**
Type deduction pushed out of `IRDb` and into the shared `Linearizer::record_expr_r`. `LinOperand` now carries a `DataType`. Arithmetic and comparison expressions fail early during linearization with robust type-mismatch diagnostic codes (e.g. `ERR_205`, `ERR_206`, `ERR_207`).

**IRDb Simplification**
`get_operand_data_type_r` entirely eliminated from `IRDb`. The `IRDb` now reads guaranteed correct types from `LinOperand::data_type()` rather than inferring them through recursive lookups, greatly improving stability and separation of concerns.

**Diagnostic Code Resolution & Test Alignment**
Cleaned up duplicate diagnostic codes across `const_eval.rs`, `linearizer.rs`, and `layoutdb.rs` after the architectural shift merged their diagnostic paths. The test harness (`tests/integration.rs`) was strictly realigned to expect typed-lowering's `[LINEAR_*]` codes instead of deferred execution `[EXEC_*]` or legacy `[IRDB_*]` codes.
