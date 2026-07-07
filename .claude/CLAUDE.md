# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

---

## AI Context

Before starting any task, read the yaml files in `docs/ai/` — they are the
authoritative context for architecture, memory, and change history:

- `01-meta.yaml` — project identity
- `02-system.yaml` — pipeline stages, extensions, language features, error codes
- `03-structure.yaml` — crate layout, key types and functions
- `04-memory.yaml` — lessons learned, active risks, open debt
- `05-update-tracker.md` — meaningful change log

After any task that modifies code or design, update the relevant yaml files and
`05-update-tracker.md` before finishing.

Keep all docs/ai files concise, factual, and easy to maintain. Do not use
passive voice except in definitions. Do not use pronouns.

---

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community
structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when
  graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for
  relationships and `graphify explain "<concept>"` for focused concepts. These
  return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw
  grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of
  raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when
  query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current
  (AST-only, no API cost).

## Commands

```bash
# Build
cargo build

# Full test suite (matches CI)
cargo test --release --all

# Single integration test by name
cargo test --release -p firmion <test_name>

# Run tests for one crate (e.g. a std extension)
cargo test -p std_xor

# Run the binary directly
cargo run -- <file.firm> -o output.bin

# Clippy lint
cargo clippy --all

# Format check
cargo fmt --all -- --check
```

The main integration suite is `tests/integration.rs` (~365 tests). Test helpers:

- `assert_firmion_success(src, output_bin, expected_output)` — expects clean
  exit and empty stderr
- `assert_firmion_failure(src, expected_err_codes)` — expects non-zero exit;
  checks stderr contains each code
- `assert_firmion_warning(src, codes)` — expects success with `-v`; checks
  stderr contains each code
- `assert_firmion_no_warning(src, codes)` — expects success with `-v`; checks
  stderr does NOT contain each code

Each test uses a derived output filename (`src.replace('/', "_") + ".bin"`) to
avoid race conditions under parallel test execution.

---

## Architecture

Firmion is a DSL compiler for composing binary image files. Source files declare sections and an output; the compiler resolves sizes, addresses, and offsets, then writes a flat binary.

### Pipeline (data flows left to right)

```text
Source text
  -> Ast / AstDb(validate=false)   [ast crate]
  -> const_eval + prune            [const_eval crate]
  -> AstDb(validate=true)          [ast crate]
  -> LayoutDb / LinIR              [layoutdb crate]
  -> IRDb                          [irdb crate]
  -> RegionDb                      [regiondb crate]
  -> LayoutPhase (iterates until stable)  [layout_phase crate]
  -> ValidationPhase (asserts)     [validation_phase crate]
  -> ExecPhase (writes bytes)      [exec_phase crate]
  -> map output                    [map_phase crate]
```

`process/process.rs` is the single orchestrator that calls every stage in order.

### Key design facts

**AstDb is built twice.** First with `validate=false` (before const_eval) to
avoid false ERR_14 positives on sections inside top-level `if/else` blocks.
Second with `validate=true` on the pruned AST.

**const_eval is an AST walker** that evaluates constants and prunes dead
`if/else` branches in one pass. It produces a cloned pruned AST.

**Typed lowering happens in the Linearizer.** `LinOperand` carries a `DataType`.
`IRDb` reads guaranteed-correct types from `LinOperand::data_type()` — it no
longer infers types. `get_operand_data_type_r` is gone.

**LayoutPhase iterates** until section sizes and addresses converge. `iterate_*`
helpers advance a `Location` cursor. `RegionDb` pre-computes `EffectiveRegion`
intersections once before iteration begins.

**ExecPhase** calls `execute_core_operations` (writes `Wr`/`Wrs`/`Wrf` bytes and
zeroed extension placeholders), then `execute_extensions` (patches extension
output in place via `OutputBuffer`).

**Extensions** implement `FirmionExtension` from the `firmion_extension` crate.
They are registered in `extensions/src/lib.rs` via feature-gated `#[cfg(feature
= "std-*")]` blocks. The feature chain is: root `Cargo.toml` -> `process` ->
`extensions` -> individual `std/*` crates. Standard extensions: `std::crc32c`,
`std::sha256`, `std::md5`, `std::xor`.

### Adding wrbe16..wrbe64 (big-endian write instructions)

`IRKind::Wr(usize)` gains a bool endianness flag: `IRKind::Wr(usize, bool)`
where `true` = big-endian. All existing `Wr` match sites change from `Wr(w)` to
`Wr(w, _)` -- the Rust compiler enforces exhaustiveness so no site is missed.
Only `exec_phase` inspects the flag. `wr8` with `big_endian: true` is a valid
encoding and behaves correctly (single byte, flag ignored).

Files to change in order:

1. `ir/ir.rs` -- change `IRKind::Wr(usize)` to `IRKind::Wr(usize, bool)`; update
   `ir_byte_size` match to `Wr(w, _)`.
2. `ast/ast.rs` -- add `LexToken::Wrbe16` ... `Wrbe64` variants; add them to
   `is_section_expr_tok`; add `"wrbe"` prefix to the `is_reserved_identifier`
   prefix check (the current rule only covers `"wr" + digit`; `wrbe` needs
   explicit handling since `"be"` is not a digit).
3. `ast/lexer.rs` -- add `"wrbe16" => LexToken::Wrbe16` ... `"wrbe64" =>
   LexToken::Wrbe64` entries in the keyword match.
4. `layoutdb/layoutdb.rs` -- add `Wrbe16` ... `Wrbe64` to the
   `is_section_expr_tok` dispatch arm.
5. `linearizer/linearizer.rs` -- update existing `LexToken::Wr8 =>
   IRKind::Wr(1)` ... entries to `IRKind::Wr(1, false)` ...; add
   `LexToken::Wrbe16 => IRKind::Wr(2, true)` ... `LexToken::Wrbe64 =>
   IRKind::Wr(8, true)`.
6. `irdb/irdb.rs` -- update `IRKind::Wr(_)` pattern to `IRKind::Wr(_, _)` in
   `validate_numeric_1_or_2`.
7. `layout_phase/layout_phase.rs` -- update `IRKind::Wr(w) => w as usize` size
   helper to `Wr(w, _)`; update `IRKind::Wr(_) => self.iterate_wrx(...)`
   dispatch to `Wr(_, _)`.
8. `exec_phase/exec_phase.rs` -- update `IRKind::Wr(w)` in `get_wrx_byte_width`
   to `Wr(w, _)`; update `IRKind::Wr(_)` dispatch to `Wr(_, _)`; in
   `execute_wrx` replace the fixed `to_le_bytes()` calls with a branch on the
   bool flag.
9. `tests/firmion_fuzz.dict` -- add `"wrbe16" "wrbe32" "wrbe64"` entries.
10. `vscode-firmion/syntaxes/firmion.tmLanguage.json` -- add
    `wrbe64|wrbe32|wrbe16` to the write-instruction regex.
11. `docs/ai/02-system.yaml` -- document the new instructions under
    `language_features`.
12. Test fixtures `tests/wrbe*.firm` and integration tests in
    `tests/integration.rs`.

---

## Technical writing rules (code comments)

- ASCII characters only -- no Unicode, curly quotes, em dashes.
- Active voice only, except in definitions.
- No pronouns.
