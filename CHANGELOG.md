# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic
Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.1-pending]

- ADDED: File and section glob capability to `obj`
- ADDED: Optional `region` `default_pad_byte` property.
- README: Added ESP32-S3 case study.
- INTERNAL: Added examples/esp32 with ESP32 image experiment code
- INTERNAL: Consolidated internal firmion libraries to about 10.

## [0.7.0] - 2026-05-05 -

This is the initial version uploaded to crates.io.

- CHANGED: Renamed 'brink' to 'firmion'
- CHANGED: restarted version enumeration with firmion-v0.7.0
- CHANGED: Unify all error codes as ERR_n
- REMOVED: Removed support for old style extension args
- ADDED: Support for `obj` external ELF format file sections.
- ADDED: Support for `obj_lma`, `obj_vma`, `obj_align`
- ADDED: std::xor extension
- ADDED: Support for big-endian writes
- ADDED: Support plain-ASCII for non-terminal output, e.g. pipes
- ADDED: `trace` command for debugging
- ADDED: std::esp_checksum extension
- FIXED: Top level print statements
- FIXED: Print and assert execution order
- FIXED: Command line string args on Windows
- FIXED: Suppress error caused by transient layout values.
- README: Added Brink features overview to the introduction
- README: Added Brink standard extensions table
- README: Execution order is now more rigorously specified
- INTERNAL: Enormous simplification of const eval to just use the AST
- INTERNAL: Type deduction moves from IRDB to the Linearizer
- INTERNAL: Replace `unsafe` mmap with OutputBuffer wrapper for vec<>
- INTERNAL: Forbid unsafe rust code

## [7.0.0] - 2026-05-02

- REMOVED: With `region`, remove obsolete starting address from `output`
  statement.
- SYNTAX CHANGE: Rename set_sec_offset to pad_sec_offset
- SYNTAX CHANGE: Rename set_addr_offset to pad_addr_offset
- SYNTAX CHANGE: Rename set_file_offset to pad_file_offset
- ADDED: Region support
- ADDED: added missing --list-extensions command line option which was already
- ADDED: support for K/M/G suffix on decimal number literals.
  documented in the README.md
- README: Removed obsolete text on if/else statement restrictions
- README: Documented region feature
- README: Improved extension documentation
- INTERNAL: Improved README.md formatting
- INTERNAL: Better division of responsibility now with distinct layout, map and
  exec phases.
- INTERNAL: Refactor parameter values into their own module.
- INTERNAL: Refactor validation as a dedicated phase.
- INTERNAL: Refactor region info into a RegionDb library
- FIXED: Fixed bad string conversion assert in verbose output
- FIXED: Fixed check for duplicate error codes and eliminated a few duplicates
- FIXED: Panic on incomplete UTF-8 character in position of an operator.

## [6.1.1] - 2026-04-20

- FIXED: Fixed version to 6.1.1 in Cargo.toml

## [6.1.0] - 2026-04-17

- ADDED: added --max-output-size CLI option.
- FIXED: Fixed assert when wrf had too man args
- FIXED: Fixed assert when wr8..wr64 had too man args
- FIXED: Fixed bad bool conversion assert
- FIXED: Fixed various unlimited recursion bugs
- FIXED: Fixed incorrect const substitution
- FIXED: Support 8MB stack depth on Windows, which matches Linux default.
- FIXED: Prevent infinite iteration in circular layout dependencies.
- INTERNAL: Added DepthGuard library for recursion limits
- INTERNAL: Better linear operand enumeration
- INTERNAL: Upgrade to latest md-5 crate.
- INTERNAL: Upgrade to latest parse_int crate.
- INTERNAL: Upgrade to latest ariadne crate
- INTERNAL: Upgrade to latest predicates crate
- INTERNAL: Upgrade to latest assert crate
- INTERNAL: Upgrade to latest serial_test crate
- INTERNAL: Remove unused codespan-reporting crate
- INTERNAL: Renamed and improved the fuzz_help.md
- INTERNAL: Added mmap test with 0 length

## [6.0.0] - 2026-04-16

- EXTENSION API BREAKING CHANGE: Generalized and unified extension parameter passing.
- ADDED: Extensions support named arguments, as in foo:bar(stuff=42);
- FIXED: Fixed to_i64() and to_u64() in const expressions
- FIXED: Use checked arithmetic for extension size.
- INTERNAL: Fixed unused import clippy warning
- INTERNAL: Cleaned up dead code in const evaluation.
- INTERNAL: Refactored common code into coerce_numeric_pair
- INTERNAL: Fixed is_none + unwrap anti-pattern.
- INTERNAL: Refactor counter advance into one function
- INTERNAL: Changed a saturating add to checked add.
- INTERNAL: Simplified extension call handling in the IR.
- INTERNAL: Implemented Rustc-style AST storage for synthetic nodes.
- INTERNAL: Added large comment about how brink handles include files.
- INTERNAL: renamed ext to extension_registry

## [5.0.6] - 2026-04-10

- Since Brink is an application, added cargo lock to github.
  The audit-check action on github requires the lockfile.
- Added std::md5 extension.
- Added a vscode syntax highlighting extension.  See the bottom of the README for instructions.
- `if/else` expressions still require const conditional evaluation, but the
  conditional blocks can now contain structural statements, e.g. wr, set_addr, etc.
- Added prebuilt binaries for homebrew

## [5.0.5] - 2026-04-09

- Internal refactoring: Replace Logos with a hand-rolled lexer.
  This reduces build dependencies and eliminated a known security vulnerability.
- Removed octal format -D defines.  Brink has never supported octal constants.
- Added security audit workflow.
- Added security audit badge to README.
- Added MIT license badge to README.

## [5.0.4] - 2026-04-09

- Automated github actions now create release binaries.

## [5.0.3] - 2026-04-09

- Trying again to automate github actions to create release binaries.

## [5.0.2] - 2026-04-09

- Trying again to automate github actions to create release binaries.

## [5.0.1] - 2026-04-09

- Trying to automate github actions to create release binaries.

## [5.0.0] - 2026-04-09

- Support for if/else in const expressions
- Support for Rust style map file output
- Many more unit tests
- std::crc32c extension
- std::sha256 extension

## [3.0.0] - 2026-03-25

- Large upgrade of Brink capability with many breaking changes.
- Completed extension infrastructure
- Completed section scoping for address and offset ranges
- Completed address overwrite detection
- Many commands have more obvious names now.

## [2.5.0] - 2026-03-25

- Support for extensions
- Global scope asserts

## [2.4.0] - 2026-03-21

- Added --map-c99 option.
- Changed --map/--map-hf to --map-csv.
- Updated --map-csv output to be more spreadsheet friendly.

## [2.3.0] - 2026-03-21

- Added include directive.

## [2.2.2] - 2026-03-20

- Resolved clippy warnings.

## [2.2.1] - 2026-03-12

- Minor human friendly map file fixes.

## [2.2.0] - 2026-03-12

- Added command line const definitions.

## [2.1.0] - 2026-03-12

- Added command line option to generate human friendly map file.

## [2.0.0] - 2026-03-12

- Added const support
- Catch use of reserved words
- Readme cleanup and new sections.

## [1.2.7] - 2026-03-12

- Alphabetize lexer tokens.

## [1.2.6] - 2026-03-12

- Extended `error_codes_are_unique` scanner to include `ir/ir.rs`.
- Renamed duplicate error code `ERR_200` (ambiguous-integer overflow) to `ERR_201`; updated `fuzz_found_10` test accordingly.
- Added tests `integer_overflow_i64` (`[ERR_201]`) and `integer_overflow_u64` (`[ERR_198]`) to cover integer literal overflow paths in `ir/ir.rs`.

## [1.2.5] - 2026-03-12

- Added `error_codes_are_unique` test that scans all source files and asserts no diagnostic error code string appears at more than one call site.
- Fixed pre-existing duplicate: `ERR_79` was used at two sites in `irdb/irdb.rs`; the second site is now `ERR_83`.

## [1.2.4] - 2026-03-12

- Pipeline stage constructors (`Ast::new`, `LinearDb::new`, `IRDb::new`, `Engine::new`) now return `Result<T, ()>` instead of `Option<T>`, enabling idiomatic `?`-based error propagation.

## [1.2.3] - 2026-03-12

- Reduce wr8...wr64 boilerplate with a parameterized enum.

## [1.2.2] - 2026-03-08

- Replaced the outdated `fern` and `log` crates with the modern `tracing` and `tracing-subscriber` ecosystem for structured CLI diagnostics.

## [1.2.1] - 2026-03-08

- Upgraded the error reporting engine to use `ariadne` for compiler diagnostics.

## [1.2.0] - 2026-03-08

- Fixed accidental integer infix operations in the Pratt parser.
- Corrected bitwise AND (`&`) and OR (`|`) operator precedence bug.  Behavior now
  matches brink documentation.

## [1.1.0] - 2026-03-07

- Refactored CLI argument parsing to use the `clap` v4 Derive API.
- Implemented Cargo Workspace Inheritance
- Fixed unchecked overflow bugs on the location counter
- Added tests to guarantee arithmetic boundary safety.

## [1.0.2] - 2026-02-25

- Updated Rust edition to 2024
- Updated dependent package versions with `cargo update`
- Fixed all `cargo clippy` warnings.

## [1.0.1] - 2021-07-05

- Warn on `rust_2018_idioms`
- Remove unused extern crate `clap` in main.rs
- Forbid unsafe Rust
- Added this changelog file

