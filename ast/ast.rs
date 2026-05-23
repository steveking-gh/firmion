// Lexer, parser and abstract syntax tree (AST) for firmion.
//
// This is the first stage of the compiler pipeline.  The lexer converts the raw
// source text into a flat token stream (LexToken). The recursive-descent /
// Pratt-expression parser then consumes that stream and builds an arena-based
// AST.  In the AST, each node holds a TokenInfo that records the token kind,
// its string value, and its byte-offset span in the source file.
//
// The astdb crate consumes the AST output and builds required lookup structures
// for later compiler phases.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

pub mod astdb;

mod lexer;
use lexer::Lexer;

use anyhow::Context;
use diags::{Diags, SourceSpan};
use indextree::{Arena, NodeId};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;

#[allow(unused_imports)]
use diags::depth_guard::{DepthGuard, MAX_RECURSION_DEPTH};
use tracing::{Level, debug, enabled, trace};

/// All tokens in firmion.
/// Keep this simple and do not be tempted to attach
/// unstructured values to these enum variants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LexToken {
    Const,
    If,
    Else,
    BuiltinOutputSize,
    BuiltinOutputAddr,
    BuiltinVersionString,
    BuiltinVersionMajor,
    BuiltinVersionMinor,
    BuiltinVersionPatch,
    Include,
    In,
    Region,
    Section,
    Align,
    PadSecOffset,
    PadAddrOffset,
    SetAddr,
    PadFileOffset,
    Assert,
    Sizeof,
    Print,
    Trace,
    ToU64,
    ToI64,
    Addr,
    AddrOffset,
    SecOffset,
    FileOffset,
    Wrs,
    Wr8,
    Wr16,
    Wr24,
    Wr32,
    Wr40,
    Wr48,
    Wr56,
    Wr64,
    Wrbe8,
    Wrbe16,
    Wrbe24,
    Wrbe32,
    Wrbe40,
    Wrbe48,
    Wrbe56,
    Wrbe64,
    Wrf,
    Wr,
    Obj,
    Output,
    DoubleEq,
    NEq,
    GEq,
    LEq,
    Gt,
    Lt,
    Eq,
    DoubleAmpersand,
    DoublePipe,
    Ampersand,
    Pipe,
    Plus,
    Minus,
    Asterisk,
    FSlash,
    Percent,
    Comma,
    DoubleLess,
    DoubleGreater,
    OpenBrace,
    CloseBrace,
    OpenParen,
    CloseParen,
    Semicolon,
    Label,
    Namespace,
    Identifier,
    Integer,
    U64,
    I64,
    QuotedString,
    /// A synthetic token marking the end of the source file. The parser may
    /// push synthetic tokens into the vector *after* the EOF token since the
    /// token vector functions as arena storage for all tokens.  Parsing stops
    /// when encountering the EOF token.
    EOF,
    /// Named argument in an extension call: `param_name=`.
    /// Not produced by the lexer; synthesized by the parser when it
    /// sees Identifier followed immediately by Eq inside a call argument
    /// list.  The token val is the parameter name (without the `=`).
    NamedArg,
    /// A property assignment inside a region block.
    /// Synthesized by parse_region_contents; tok = RegionProp, val = property
    /// name ("addr" or "size").
    /// The single child is the expression value.
    RegionProp,
    /// Section-to-region binding recorded during parse_section.
    /// Synthesized when `section NAME in REGION` is parsed; tok = RegionRef,
    /// val = region name.  No children.
    RegionRef,
    /// A property inside an obj block.
    /// Synthesized by parse_obj; tok = ObjProp, val = property name
    /// ("section" or "file").
    /// The single child is the QuotedString value.
    ObjProp,
    /// obj_align(<obj>) -- returns section alignment as U64.
    ObjAlign,
    /// obj_lma(<obj>) -- returns section LMA as U64 (ELF only).
    ObjLma,
    /// obj_vma(<obj>) -- returns section VMA as U64.
    ObjVma,
    /// Catch-all for unrecognized input.
    Unknown,
}

/// Returns true if `name` is a reserved identifier that may not be used
/// as a section name, const name, or label name.
///
/// Reserved prefixes:
///   - "wr" + digit  — write instructions (wr8, wr16, wr32, etc)
///   - "__"          — builtin identifiers (__output_size, __output_addr, etc)
///
/// Reserved exact keywords:
///   - "wr" / "wrs" / "wrf"       — write commands
///   - "section" / "output"       — structural declarations
///   - "const" / "region" / "in"  — structural declarations
///   - "align"                    — alignment directive
///   - "assert"                   — assertion
///   - "sizeof" / "addr"          — address and size expressions
///   - "addr_offset" / "sec_offset" / "file_offset" — offset expressions
///   - "print"                    — debug output
///   - "to_u64" / "to_i64"        — type conversion
///   - "include" / "import"       — file inclusion
///   - "if" / "else"              — conditional inclusion
///   - "true" / "false"           — boolean literals
///   - "extern" / "let" / "fill"  — reserved for future use
pub fn is_reserved_identifier(name: &str) -> bool {
    // "wr" followed by at least one digit reserves the numeric write variants.
    if let Some(rest) = name.strip_prefix("wr")
        && rest.starts_with(|c: char| c.is_ascii_digit())
    {
        return true;
    }
    // "wrbe" followed by at least one digit reserves big-endian write variants.
    if let Some(rest) = name.strip_prefix("wrbe")
        && rest.starts_with(|c: char| c.is_ascii_digit())
    {
        return true;
    }
    if name.starts_with("__") {
        return true;
    }
    matches!(
        name,
        "wr" | "wrs"
            | "wrf"
            | "section"
            | "output"
            | "const"
            | "region"
            | "in"
            | "align"
            | "assert"
            | "sizeof"
            | "addr"
            | "addr_offset"
            | "sec_offset"
            | "file_offset"
            | "pad_addr_offset"
            | "pad_sec_offset"
            | "pad_file_offset"
            | "set_addr"
            | "print"
            | "trace"
            | "to_u64"
            | "to_i64"
            | "include"
            | "import"
            | "if"
            | "else"
            | "true"
            | "false"
            | "extern"
            | "let"
            | "fill"
            | "obj_align"
            | "obj_lma"
            | "obj_vma"
            | "obj"
    )
}

/// Returns true if the token is one that has a meaningful value
/// to show in debug logs.
pub const fn has_useful_debug_value(tok: LexToken) -> bool {
    matches!(
        tok,
        LexToken::QuotedString
            | LexToken::Identifier
            | LexToken::Namespace
            | LexToken::Integer
            | LexToken::U64
            | LexToken::I64
            | LexToken::NamedArg
            | LexToken::RegionProp
            | LexToken::RegionRef
            | LexToken::ObjProp
    )
}

/// Logs a token at DEBUG level via TokenVector::describe_token, skipping the
/// call entirely when DEBUG is disabled.  offset is relative to the current
/// cursor: 0 = current token, -1 = previous token, etc.
macro_rules! debug_peek {
    ($src_str:expr, $tv:expr) => {
        if enabled!(Level::DEBUG) {
            debug!("{}: {}", $src_str, $tv.describe_token_at_offset(0));
        }
    };
}

/// The basic token info structure used everywhere. The AST constructs a vector
/// of TokenInfos.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenInfo<'toks> {
    /// The token as identified by the lexer.
    pub tok: LexToken,

    /// The range of bytes in the source file occupied by this token.
    /// Diagnostics require this range when producing errors.
    pub loc: SourceSpan,

    /// The value of the token trimmed of whitespace
    pub val: &'toks str,
}

impl<'toks> TokenInfo<'toks> {
    pub fn span(&self) -> SourceSpan {
        self.loc.clone()
    }
}

/// The token vector abstraction used in the AST.  TokenVector proves a thin wrapper around
/// the underlying vector of TokenInfos and provides basic access function that pay
/// attention to the EOF sentinel.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenVector<'toks> {
    tv: Vec<TokenInfo<'toks>>,
    idx: usize,
}

impl<'toks> TokenVector<'toks> {
    pub fn new(mut tv: Vec<TokenInfo<'toks>>) -> Self {
        tv.push(TokenInfo {
            tok: LexToken::EOF,
            val: "",
            loc: SourceSpan {
                file_id: 0,
                range: 0..0,
            },
        });
        Self { tv, idx: 0 }
    }

    pub fn get_index(&self) -> usize {
        self.idx
    }

    pub fn scannable_len(&self) -> usize {
        self.tv.len() - 1
    }

    /// Get needs access to all tokens.
    pub fn get(&self, idx: usize) -> &TokenInfo<'toks> {
        self.tv.get(idx).unwrap_or_else(|| {
            panic!(
                "TokenVector::get: Index {} out of bounds, tv length={}",
                idx,
                self.tv.len()
            )
        })
    }

    /// Return the next unread token, but do not advance.  The sequential parser
    /// never needs to peek past the end-of-input.
    pub fn peek(&self) -> &TokenInfo<'toks> {
        &self.tv[self.idx]
    }

    /// Return the next unread token and advance.  Take stops at the end of input.
    pub fn take(&mut self) -> &TokenInfo<'toks> {
        let tinfo = &self.tv[self.idx];
        if self.idx < self.tv.len() - 1 {
            self.idx += 1;
        }
        tinfo
    }

    /// Return a human-readable description of the token at index.
    /// Out-of-bounds or EOF positions are described gracefully rather than panicking.
    /// This is a debug helper function that needs access to all tokens.
    pub fn describe_token(&self, idx: usize) -> String {
        match self.tv.get(idx) {
            None => format!("token {} out of bounds, length={}", idx, self.tv.len()),
            Some(tinfo) if has_useful_debug_value(tinfo.tok) => {
                format!("token {}:{:?} is {:?}", idx, tinfo.tok, tinfo.val)
            }
            Some(tinfo) => format!("token {}:{:?}", idx, tinfo.tok),
        }
    }

    /// Return a human-readable description of the token at index + offset.
    /// Negative offsets look at previously taken tokens.  Out-of-bounds or EOF
    /// positions are described gracefully rather than panicking. This is a
    /// debug helper function that needs access to all tokens.
    pub fn describe_token_at_offset(&self, offset: isize) -> String {
        let idx = self.idx as isize + offset;
        if idx < 0 {
            return format!("token {:+} out of bounds, current index={}", idx, self.idx);
        }
        self.describe_token(idx as usize)
    }

    // Skip stops at the end of input.
    pub fn skip(&mut self) {
        if self.idx >= self.tv.len() {
            panic!(
                "TokenVector::skip: Index {} exceeded bounds, tv length={}",
                self.idx,
                self.tv.len()
            )
        }
        if self.idx < self.tv.len() - 1 {
            self.idx += 1;
        }
    }

    /// Returns the current token index and advances the cursor to the next token.
    /// Does not advance past EOF
    pub fn get_index_and_skip(&mut self) -> usize {
        let idx = self.idx;
        self.skip();
        idx
    }
}

/**
 * Abstract Syntax Tree
 * This structure contains the AST created from the raw lexical
 * tokens.  The lifetime of this struct is the same as the tokens.
 */
#[derive(Clone)]
pub struct Ast<'toks> {
    /// The arena from the indextree crate holding all nodes
    /// in the AST.  Arenas are one idiomatic rust way to nicely
    /// manage the pointer craziness of trees
    arena: Arena<TokenInfo<'toks>>,

    /// A vector of info about for tokens identified by logos.
    tv: TokenVector<'toks>,

    /// The artificial root of the tree.  The children of this
    /// tree are the top level tokens in the user's source file.
    root: NodeId,
}

/// Context passed through parse_if / parse_if_body to control which
/// statements are legal inside an if/else body.
#[derive(Clone, Copy)]
enum ParseIfContext {
    /// if/else at the top level or nested inside another const if/else.
    /// Only const-compatible statements are allowed.
    TopLevel,
    /// if/else directly inside a section body.
    /// All section-level statements are allowed in addition to const ones.
    Section,
}

impl<'toks> Ast<'toks> {
    /// Recursively lexes the provided `fstr` source string, flattening the tokens directly
    /// into the main `tv` stream. Encountering an `include "path";` sequence triggers
    /// path resolution relative to the current file, reading the target contents,
    /// registering the path with `diags` to claim a new `file_id`, and recursing
    /// to insert the nested tokens into the stream.
    ///
    /// The `visited` hash map tracks canonical paths to prevent include cycles.
    fn lex_file_r(
        tv: &mut Vec<TokenInfo<'toks>>,
        name: &str,
        fstr: &'toks str,
        file_id: usize,
        diags: &mut Diags,
        visited: &mut HashMap<String, SourceSpan>,
    ) -> anyhow::Result<()> {
        let mut lex = Lexer::new(fstr);
        while let Some(tok) = lex.next() {
            let val = lex.slice();
            if enabled!(Level::DEBUG) {
                if has_useful_debug_value(tok) {
                    debug!("ast::lex_file_r: token {} = {:?} {:?}", tv.len(), tok, val);
                } else {
                    debug!("ast::lex_file_r: token {} = {:?}", tv.len(), tok);
                }
            }

            let span = lex.span();
            if tok == LexToken::Include {
                // How Firmion handles include files:
                //
                // First, unlike the C preprocessor, Firmion does not literally
                // text substitute the included file into the parent.  Instead,
                // we logically inline the tokens from the included file into
                // our single unified token vector (tv). The token number of
                // each token in the tv increases monotonically regardless if
                // the token belonged to a parent or included file.  The
                // downstream parser sees a simple flat vector. For diagnostics,
                // we track the original source file and source line number of
                // each token. Because the inlining is not text substitution, we
                // avoid C preprocessor style line number fixups.
                //
                // Secondly, we do not record the LexToken::Include nor the file
                // path LexToken::QuotedString in the tv. Because the lexer
                // already logically inlined the included file's tokens, The
                // LexToken::Include and LexToken::QuotedString are "consumed"
                // and would just be noise downstream.
                //
                // Finally, because of this logical inlining, we do basic
                // semantic checks here to enforce that LexToken::Include is
                // used in a valid way. This semantic check determines whether
                // 'include' serves as a top-level directive rather than misused
                // token. Checking whether 'include'appears immediately after a
                // statement boundary (or at the start of the file) prevents
                // eagerly intercepting valid parser-level error cases like
                // ERR_27 (Reserved section name) or ERR_28 (Reserved const
                // name).
                let is_directive = tv
                    .last()
                    .is_none_or(|t| matches!(t.tok, LexToken::Semicolon | LexToken::CloseBrace));

                if is_directive {
                    let next_tok = lex.next();
                    if next_tok != Some(LexToken::QuotedString) {
                        diags.err1(
                            "ERR_29",
                            "Expected quoted string after include",
                            SourceSpan {
                                file_id,
                                range: span,
                            },
                        );
                        anyhow::bail!("AST lexing failed");
                    }
                    let path_val = lex.slice();
                    let path_span = lex.span();

                    let semi_tok = lex.next();
                    if semi_tok != Some(LexToken::Semicolon) {
                        diags.err1(
                            "ERR_30",
                            "Expected semicolon after include statement",
                            SourceSpan {
                                file_id,
                                range: path_span,
                            },
                        );
                        anyhow::bail!("AST lexing failed");
                    }

                    let raw_path = path_val
                        .strip_prefix('"')
                        .unwrap()
                        .strip_suffix('"')
                        .unwrap();
                    let base_dir = std::path::Path::new(name)
                        .parent()
                        .unwrap_or(std::path::Path::new(""));
                    let resolved_path = base_dir.join(raw_path);

                    // Use a normalized string path for cycle detection
                    let resolved_path_str = if let Ok(c) = resolved_path.canonicalize() {
                        c.to_string_lossy().to_string()
                    } else {
                        // Fallback to basic string if it doesn't exist to generate good error
                        resolved_path.to_string_lossy().to_string()
                    };

                    if let Some(orig_span) = visited.get(&resolved_path_str) {
                        diags.err2(
                            "ERR_31",
                            &format!("Include cycle detected: {}", resolved_path_str),
                            SourceSpan {
                                file_id,
                                range: span.clone(),
                            },
                            orig_span.clone(),
                        );
                        anyhow::bail!("AST lexing failed");
                    }
                    visited.insert(
                        resolved_path_str.clone(),
                        SourceSpan {
                            file_id,
                            range: span.clone(),
                        },
                    );

                    let content = match std::fs::read_to_string(&resolved_path) {
                        Ok(c) => c,
                        Err(e) => {
                            diags.err1(
                                "ERR_32",
                                &format!(
                                    "Failed to read included file '{}': {}",
                                    resolved_path_str, e
                                ),
                                SourceSpan {
                                    file_id,
                                    range: path_span,
                                },
                            );
                            anyhow::bail!("AST lexing failed");
                        }
                    };

                    let leaked_content: &'toks str = Box::leak(content.into_boxed_str());
                    let inc_file_id = diags.add_file(&resolved_path_str, leaked_content);

                    Self::lex_file_r(
                        tv,
                        &resolved_path_str,
                        leaked_content,
                        inc_file_id,
                        diags,
                        visited,
                    )?;

                    continue;
                }
            }

            tv.push(TokenInfo {
                tok,
                val,
                loc: SourceSpan {
                    file_id,
                    range: span,
                },
            });
        }
        Ok(())
    }

    /// Create a new abstract syntax tree.
    pub fn new(name: &str, fstr: &'toks str, diags: &mut Diags) -> anyhow::Result<Self> {
        let mut arena = Arena::new();
        let dummy_tinfo = TokenInfo {
            tok: LexToken::EOF,
            val: "",
            loc: SourceSpan {
                file_id: 0,
                range: 0..0,
            },
        };
        let root = arena.new_node(dummy_tinfo);
        let mut raw_tv = Vec::new();
        let mut visited = HashMap::new();

        // In Phase 1, `process.rs` adds the main file to diags at id=0.
        // We reuse that knowledge here.
        let main_path = if let Ok(c) = std::path::Path::new(name).canonicalize() {
            c.to_string_lossy().to_string()
        } else {
            name.to_string()
        };
        visited.insert(
            main_path,
            SourceSpan {
                file_id: 0,
                range: 0..0,
            },
        );

        // Recursively lex the main file and all includes, flattening tokens
        // into a single vector.
        Self::lex_file_r(&mut raw_tv, name, fstr, 0, diags, &mut visited)?;
        let mut ast = Self {
            arena,
            tv: TokenVector::new(raw_tv),
            root,
        };

        // Now parse the flat token vector to build the abstract syntax tree.
        if !ast.parse(diags) {
            // ast construction failed.  Let the caller report
            // this in whatever way they want.
            anyhow::bail!("AST construction failed.");
        }

        Ok(ast)
    }

    // Boilerplate entry debug tracing for recursive descent parsing functions.
    fn dbg_enter(&self, func_name: &str) {
        if enabled!(Level::TRACE) {
            trace!(
                "Ast::{} ENTER, {}",
                func_name,
                self.tv.describe_token_at_offset(0)
            );
        }
    }

    /// Boilerplate exit debug tracing for recursive descent parsing functions.
    /// This function returns the result and should be the last statement
    /// in each function
    fn dbg_exit(&self, func_name: &str, result: bool) -> bool {
        trace!("Ast::{} EXIT {:?}", func_name, result);
        result
    }

    /// Boilerplate exit debug tracing for pratt parsing functions.
    /// This function returns the result and should be the last statement
    /// in each function
    fn dbg_exit_pratt(&self, func_name: &str, top: &Option<NodeId>, result: bool) -> bool {
        trace!(
            "Ast::{} EXIT {} with node id {:?}",
            func_name,
            if result { "OK" } else { "!! FAIL !!" },
            top
        );
        result
    }

    /// Return an iterator over the children of the specified AST node
    pub fn children(&self, nid: NodeId) -> indextree::Children<'_, TokenInfo<'toks>> {
        nid.children(&self.arena)
    }

    /// Returns true if the specified node has child nodes
    pub fn has_children(&self, nid: NodeId) -> bool {
        nid.children(&self.arena).next().is_some()
    }

    /// Returns the lexical value of the specified child of the specified
    /// parent. The value is always a string reference to source code regardless
    /// of the semantic meaning of the child.
    pub fn get_child_str(&'toks self, parent_nid: NodeId, child_num: usize) -> Option<&'toks str> {
        debug!(
            "Ast::get_child_str: child number {} for parent nid {}",
            child_num, parent_nid
        );
        let mut children = parent_nid.children(&self.arena);
        if let Some(name_nid) = children.nth(child_num) {
            let tinfo = self.get_tinfo(name_nid);
            return Some(tinfo.val);
        }
        None
    }

    /// Parse the flat token vector to build the syntax tree. Unlike the flat
    /// vector of tokens, the tree represents the semantic parent-child relation
    /// between elements in the source file.  We check syntax and grammar during
    /// tree construction.
    fn parse(&mut self, diags: &mut Diags) -> bool {
        debug!(
            "Ast::parse: ENTER, Total of {} tokens",
            self.tv.scannable_len()
        );

        let mut result = true;
        while let tinfo = self.tv.peek()
            && tinfo.tok != LexToken::EOF
        {
            debug_peek!("Ast::parse", self.tv);
            result &= match tinfo.tok {
                LexToken::Section => self.parse_section(self.root, diags),
                LexToken::Region => self.parse_region(self.root, diags),
                LexToken::Obj => self.parse_obj(self.root, diags),
                LexToken::Output => self.parse_output(self.root, diags),
                LexToken::Const => self.parse_const(self.root, diags),
                LexToken::If => self.parse_if_r(self.root, diags, ParseIfContext::TopLevel),
                LexToken::Identifier => {
                    let ok = self.parse_deferred_assign(self.root, diags);
                    if !ok {
                        // If parsing of an identifier fails, consume the whole statement
                        // to avoid sending bad tokens to later phases.  Reason being
                        // an unknown identifier can be all sort of arbitrary chars and
                        // is hard to make sense of later on.
                        self.advance_past_semicolon();
                    }
                    ok
                }
                LexToken::Assert => {
                    // Global assert: evaluated in the validation phase after all
                    // sections and extensions are written.
                    let ok = self.parse_expr(self.root, diags);
                    if !ok {
                        self.advance_past_semicolon();
                    }
                    ok
                }
                LexToken::Print | LexToken::Trace => {
                    let ok = self.parse_expr(self.root, diags);
                    if !ok {
                        self.advance_past_semicolon();
                    }
                    ok
                }

                // Unrecognized top level token.  Report the error, but keep going
                // to try to give the user more errors in batches.
                _ => {
                    let msg = format!("Unrecognized token '{}' at top level scope", tinfo.val);
                    diags.err1("ERR_16", &msg, tinfo.span());

                    // Skip the bad token.
                    self.tv.skip();
                    false
                }
            };
        }
        self.dbg_exit("parse", result)
    }

    /// Helper function for errors in which the parser failed to find the
    /// expected next token
    fn err_expected_after(&self, diags: &mut Diags, code: &str, msg: &str) {
        let idx = self.tv.get_index();
        let tinfo_after = self.tv.get(idx);
        let tinfo_before = self.tv.get(idx - 1);
        let m = format!("{}, found {}", msg, self.tv.describe_token(idx));
        diags.err2(code, &m, tinfo_after.span(), tinfo_before.span());
    }

    fn err_invalid_expression(&self, diags: &mut Diags, code: &str) {
        let tinfo = self.tv.get(self.tv.get_index());
        let m = format!("Invalid expression '{}'", tinfo.val);
        diags.err1(code, &m, tinfo.span());
    }

    fn err_no_input(&self, diags: &mut Diags) {
        diags.err0("ERR_12", "Unexpected end of input");
    }

    fn err_no_close_brace(&self, diags: &mut Diags, brace_tok_num: usize) {
        let m = "Missing '}'.  The following open brace is unmatched.".to_string();
        diags.err1("ERR_13", &m, self.tv.get(brace_tok_num).span());
    }

    /// Attempts to advance the token number past the next semicolon. The final
    /// token number may be invalid.  This function is used to try to recover
    /// from syntax errors.
    ///
    /// If the current token is already one past a semicolon, then do nothing.
    /// This case occurs when the semicolon itself was unexpected, e.g. missing
    /// close paren like assert(1;
    fn advance_past_semicolon(&mut self) {
        self.dbg_enter("advance_past_semicolon");
        let prev_tinfo = self.tv.get(self.tv.get_index() - 1);
        if prev_tinfo.tok != LexToken::Semicolon {
            loop {
                let tinfo = self.tv.take();
                if tinfo.tok == LexToken::EOF || tinfo.tok == LexToken::Semicolon {
                    break;
                }
            }
        }
        debug!(
            "Ast::advance_past_semicolon: Stopped on token {}",
            self.tv.get_index()
        );
        self.dbg_exit("advance_past_semicolon", true);
    }

    /// Add the specified token as a child of the parent.
    /// Advance the token number and return the new node ID for the input token.
    fn add_to_parent_and_advance(&mut self, parent: NodeId) -> NodeId {
        let tinfo = self.tv.take().clone();
        let nid = self.arena.new_node(tinfo);
        parent.append(nid, &mut self.arena);
        nid
    }

    fn expect_leaf(
        &mut self,
        diags: &mut Diags,
        parent: NodeId,
        expected_token: LexToken,
        code: &str,
        context: &str,
    ) -> bool {
        self.dbg_enter("expect_leaf");

        let mut result = false;

        let tinfo = self.tv.peek();
        if tinfo.tok == LexToken::EOF {
            self.err_no_input(diags);
        } else if expected_token == tinfo.tok {
            self.add_to_parent_and_advance(parent);
            result = true;
        } else {
            self.err_expected_after(diags, code, context);
        }

        self.dbg_exit("expect_leaf", result)
    }

    /// Process an expected semicolon.  This function is just a convenient
    /// specialization of expect_leaf().
    fn expect_semi(&mut self, diags: &mut Diags, parent: NodeId) -> bool {
        let tinfo = self.tv.peek();

        if tinfo.tok == LexToken::EOF {
            self.err_no_input(diags);
            return false;
        }

        if LexToken::Semicolon == tinfo.tok {
            self.add_to_parent_and_advance(parent);
            return true;
        }
        self.err_expected_after(diags, "ERR_15", "Expected ';'");
        false
    }

    /// Expect the specified token, add it to the parent and advance.
    fn expect_token(&mut self, expected_tok: LexToken, diags: &mut Diags, parent: NodeId) -> bool {
        let tinfo = self.tv.peek();
        if tinfo.tok == LexToken::EOF {
            self.err_no_input(diags);
            return false;
        }

        if expected_tok == tinfo.tok {
            self.add_to_parent_and_advance(parent);
            return true;
        }

        let msg = format!("Expected {:?}", expected_tok);
        self.err_expected_after(diags, "ERR_23", &msg);
        false
    }

    /// Expect an expression which cannot be None.
    fn expect_expr(&mut self, parent: NodeId, diags: &mut Diags) -> bool {
        let mut expr_opt = None;
        if !self.parse_pratt(0, &mut expr_opt, diags) {
            return false;
        }
        let Some(expr) = expr_opt else {
            let tinfo = self.get_tinfo(parent);
            let msg = format!(
                "Expected valid expression inside parentheses after {:?}",
                tinfo.tok
            );
            diags.err1("ERR_11", &msg, tinfo.span());
            return false;
        };
        // Success, add the expression a child of the input parent node.
        parent.append(expr, &mut self.arena);
        true
    }

    /// Expect zero or one instance of specified tokens.
    /// If we find an allowed found, add it to the parent and advance.
    /// If not found, do nothing and return success
    fn optional_token(&mut self, tokvec: &[LexToken], diags: &mut Diags, parent: NodeId) -> bool {
        let tinfo = self.tv.peek();
        if tinfo.tok == LexToken::EOF {
            self.err_no_input(diags);
            return true;
        }

        if tokvec.contains(&tinfo.tok) {
            self.add_to_parent_and_advance(parent);
        }
        true
    }

    /// Accept any word-like token as a name and add it as a child of parent.
    /// Unlike expect_leaf(Identifier), this also accepts keyword tokens (e.g.
    /// LexToken::Include) so that AstDb can produce a reserved-identifier error
    /// instead of a generic parse error.  Any token whose val matches the
    /// identifier pattern [a-zA-Z_][a-zA-Z0-9_]* is accepted.
    fn expect_name_leaf(
        &mut self,
        diags: &mut Diags,
        parent: NodeId,
        code: &str,
        context: &str,
    ) -> bool {
        self.dbg_enter("expect_name_leaf");
        let tinfo = self.tv.peek();
        if tinfo.tok == LexToken::EOF {
            self.err_no_input(diags);
            return self.dbg_exit("expect_name_leaf", false);
        }
        // Accept any token whose val looks like an identifier so that AstDb can
        // provide a reserved-identifier error rather than a generic parse failure.
        let val_is_name = {
            let mut chars = tinfo.val.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if val_is_name {
            self.add_to_parent_and_advance(parent);
            return self.dbg_exit("expect_name_leaf", true);
        }
        self.err_expected_after(diags, code, context);
        self.dbg_exit("expect_name_leaf", false)
    }

    /// Expect the specified token and advance without adding to the parent.
    fn expect_token_no_add(&mut self, tok: LexToken, diags: &mut Diags) -> bool {
        let tinfo = self.tv.peek();
        if tinfo.tok == LexToken::EOF {
            self.err_no_input(diags);
            return false;
        }
        if tok == tinfo.tok {
            self.tv.skip();
            return true;
        }

        let msg = format!("Expected {:?}", tok);
        self.err_expected_after(diags, "ERR_18", &msg);
        false
    }

    /// Parses a `region` declaration and attaches it to the AST.
    ///
    /// ```text
    /// region <name> { <statements> }
    ///
    ///   region              <- root node for a region declaration
    ///   ├── <Identifier>     <- region name
    ///   ├── {                <- syntactic delimiter, marks start of body
    ///   ├── addr
    ///   ├── size
    ///   └── }                <- syntactic delimiter, marks end of body
    /// ```
    fn parse_region(&mut self, parent: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_region");
        let mut result = false;
        // Regions are always children of the root node, but no need to make
        // that a special case here.
        let reg_nid = self.add_to_parent_and_advance(parent);

        // After 'region' an identifier is expected.  expect_name_leaf also accepts
        // keyword tokens so AstDb can emit the specific reserved-identifier error.
        if self.expect_name_leaf(
            diags,
            reg_nid,
            "ERR_53",
            "Expected an identifier after region",
        ) {
            // After a region identifier, expect an open brace.
            // Remember the location of the opening brace to help with
            // user missing brace errors.
            let brace_toknum = self.tv.get_index();
            if self.expect_leaf(
                diags,
                reg_nid,
                LexToken::OpenBrace,
                "ERR_54",
                "Expected { after region name",
            ) {
                result = self.parse_region_contents(reg_nid, diags, brace_toknum);
            }
        }
        self.dbg_exit("parse_region", result)
    }

    /// Parses an `obj` declaration at the top level.
    ///
    /// ```text
    /// obj <name> {
    ///     section = "<elf-section>";
    ///     file    = "<file-path>";
    /// }
    ///
    ///   obj                    <- root node
    ///   ├── <Identifier>        <- declared name
    ///   ├── ObjProp("section")  <- section property node
    ///   │   └── <QuotedString>  <- ELF section name
    ///   └── ObjProp("file")     <- file property node
    ///       └── <QuotedString>  <- file path
    /// ```
    /// Properties may appear in any order; both are required.
    fn parse_obj(&mut self, parent: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_obj");
        let obj_nid = self.add_to_parent_and_advance(parent);

        if !self.expect_name_leaf(
            diags,
            obj_nid,
            "ERR_61",
            "Expected an identifier after 'obj'",
        ) {
            return self.dbg_exit("parse_obj", false);
        }

        if self.tv.peek().tok != LexToken::OpenBrace {
            self.err_expected_after(diags, "ERR_61", "'obj <name>': expected '{'");
            return self.dbg_exit("parse_obj", false);
        }
        let brace_tok_num = self.tv.get_index();
        self.tv.skip(); // consume '{'

        let mut section_seen = false;
        let mut file_seen = false;

        loop {
            let tinfo = self.tv.peek();
            if tinfo.tok == LexToken::CloseBrace {
                self.tv.skip();
                break;
            }
            if tinfo.tok == LexToken::EOF {
                self.err_no_close_brace(diags, brace_tok_num);
                return self.dbg_exit("parse_obj", false);
            }

            let prop_name = tinfo.val;
            let prop_loc = tinfo.span();

            let is_section = match prop_name {
                "section" => true,
                "file" => false,
                _ => {
                    let m = format!(
                        "Unknown obj property '{}'. Expected 'section' or 'file'.",
                        prop_name
                    );
                    diags.err1("ERR_70", &m, prop_loc);
                    return self.dbg_exit("parse_obj", false);
                }
            };

            if (is_section && section_seen) || (!is_section && file_seen) {
                let m = format!("Duplicate '{}' property in obj block.", prop_name);
                diags.err1("ERR_69", &m, prop_loc);
                return self.dbg_exit("parse_obj", false);
            }

            self.tv.skip(); // consume property name

            if self.tv.peek().tok != LexToken::Eq {
                self.err_expected_after(diags, "ERR_67", &format!("'{}': expected '='", prop_name));
                return self.dbg_exit("parse_obj", false);
            }
            self.tv.skip(); // consume '='

            let next = self.tv.peek().tok;
            if next != LexToken::QuotedString && next != LexToken::Identifier {
                self.err_expected_after(
                    diags,
                    "ERR_68",
                    &format!(
                        "'{} =': expected a quoted string value or const identifier",
                        prop_name
                    ),
                );
                return self.dbg_exit("parse_obj", false);
            }

            // Synthesize an ObjProp node whose val is the property name,
            // then attach the QuotedString value as its child.
            let prop_node = TokenInfo {
                tok: LexToken::ObjProp,
                loc: prop_loc,
                val: prop_name,
            };
            let prop_nid = self.arena.new_node(prop_node);
            obj_nid.append(prop_nid, &mut self.arena);
            self.parse_leaf(prop_nid); // attaches QuotedString child and advances

            if self.tv.peek().tok != LexToken::Semicolon {
                self.err_expected_after(diags, "ERR_72", "Expected ';' after obj property value");
                return self.dbg_exit("parse_obj", false);
            }
            self.tv.skip(); // consume ';'

            if is_section {
                section_seen = true;
            } else {
                file_seen = true;
            }
        }

        if !section_seen || !file_seen {
            let missing = if !section_seen { "section" } else { "file" };
            let m = format!("obj block is missing required property '{}'.", missing);
            let name_nid = obj_nid.children(&self.arena).next().unwrap();
            let name_loc = self.arena[name_nid].get().loc.clone();
            diags.err1("ERR_71", &m, name_loc);
            return self.dbg_exit("parse_obj", false);
        }

        self.dbg_exit("parse_obj", true)
    }

    /// Parses the body of a `region` block: `name = expr ;` assignments until `}`.
    fn parse_region_contents(
        &mut self,
        reg_nid: NodeId,
        diags: &mut Diags,
        brace_toknum: usize,
    ) -> bool {
        self.dbg_enter("parse_region_contents");
        let mut result = true;
        let mut seen_addr = false;
        let mut seen_size = false;
        let mut seen_default_pad = false;

        loop {
            let tinfo = self.tv.peek();

            if tinfo.tok == LexToken::EOF {
                self.err_no_close_brace(diags, brace_toknum);
                return self.dbg_exit("parse_region_contents", false);
            }

            if tinfo.tok == LexToken::CloseBrace {
                self.add_to_parent_and_advance(reg_nid);
                break;
            }

            let prop_val = tinfo.val;
            let prop_loc = tinfo.span(); // owned SourceSpan; tinfo borrow ends here

            let mut duplicate_property = false;

            match prop_val {
                "addr" => {
                    if seen_addr {
                        duplicate_property = true;
                    }
                    seen_addr = true;
                }
                "size" => {
                    if seen_size {
                        duplicate_property = true;
                    }
                    seen_size = true;
                }
                "default_pad_byte" => {
                    if seen_default_pad {
                        duplicate_property = true;
                    }
                    seen_default_pad = true;
                }
                _ => {
                    let msg = format!(
                        "Unknown region property '{}'; expected addr, size, or default_pad_byte",
                        prop_val
                    );
                    diags.err1("ERR_40", &msg, prop_loc);
                    self.tv.skip();
                    self.advance_past_semicolon();
                    result = false;
                    continue;
                }
            }

            if duplicate_property {
                let msg = format!("Duplicate region property '{}'", prop_val);
                diags.err1("ERR_41", &msg, prop_loc);
                self.tv.skip();
                self.advance_past_semicolon();
                result = false;
                continue;
            }

            if !self.parse_region_property(reg_nid, diags, prop_val, prop_loc) {
                result = false;
            }
        }

        // Verify required properties are present.
        let reg_tinfo = self.get_tinfo(reg_nid);
        if !seen_addr {
            diags.err1(
                "ERR_42",
                "Region is missing required property 'addr'",
                reg_tinfo.span(),
            );
            result = false;
        }
        if !seen_size {
            diags.err1(
                "ERR_59",
                "Region is missing required property 'size'",
                reg_tinfo.span(),
            );
            result = false;
        }

        self.dbg_exit("parse_region_contents", result)
    }

    /// Parses `= expr ;` for one validated, non-duplicate region property.
    /// Consumes the property name token, then expects `=`, an expression, and `;`.
    /// On success, appends a RegionProp node (with the expression as a child) to reg_nid.
    fn parse_region_property(
        &mut self,
        reg_nid: NodeId,
        diags: &mut Diags,
        prop_val: &'toks str,
        prop_loc: SourceSpan,
    ) -> bool {
        self.dbg_enter("parse_region_property");
        self.tv.skip(); // consume property name

        let eq_tinfo = self.tv.peek();
        if eq_tinfo.tok == LexToken::EOF {
            self.err_no_input(diags);
            return self.dbg_exit("parse_region_property", false);
        }
        if eq_tinfo.tok != LexToken::Eq {
            let msg = format!("Expected '=' after region property '{}'", prop_val);
            diags.err1("ERR_57", &msg, eq_tinfo.span());
            self.advance_past_semicolon();
            return self.dbg_exit("parse_region_property", false);
        }
        self.tv.skip(); // consume '='

        // Synthesize a RegionProp node.  Children: expression root, then ';'.
        let prop_node = TokenInfo {
            tok: LexToken::RegionProp,
            loc: prop_loc,
            val: prop_val,
        };
        let prop_nid = self.arena.new_node(prop_node);
        reg_nid.append(prop_nid, &mut self.arena);

        if !self.expect_expr(prop_nid, diags) {
            self.advance_past_semicolon();
            return self.dbg_exit("parse_region_property", false);
        }

        let ok = self.expect_semi(diags, prop_nid);
        self.dbg_exit("parse_region_property", ok)
    }

    /// Parses a `section` declaration and attaches it to the AST.
    ///
    /// ```text
    /// section <name> { <statements> }
    ///
    ///   section              <- root node for a section declaration
    ///   ├── <Identifier>     <- section name
    ///   ├── {                <- syntactic delimiter, marks start of body
    ///   ├── [statements...]  <- zero or more content nodes (see parse_section_contents)
    ///   └── }                <- syntactic delimiter, marks end of body
    /// ```
    fn parse_section(&mut self, parent: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_section");
        let mut result = false;
        // Sections are always children of the root node, but no need to make
        // that a special case here.
        let sec_nid = self.add_to_parent_and_advance(parent);

        // After 'section' an identifier is expected.  expect_name_leaf also accepts
        // keyword tokens so AstDb can emit the specific reserved-identifier error.
        if self.expect_name_leaf(
            diags,
            sec_nid,
            "ERR_1",
            "Expected an identifier after section",
        ) {
            // Optional `in REGION` binding between the name and opening brace.
            let peek = self.tv.peek();
            if peek.tok == LexToken::In {
                self.tv.skip(); // consume 'in'
                let tinfo = self.tv.peek();
                if tinfo.tok == LexToken::EOF {
                    self.err_no_input(diags);
                    return self.dbg_exit("parse_section", false);
                }
                // Accept any word-like token so AstDb can produce a better error
                // for reserved names; reject clearly non-identifier tokens.
                let val_is_name = {
                    let mut chars = tinfo.val.chars();
                    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                };
                if !val_is_name {
                    self.err_expected_after(
                        diags,
                        "ERR_44",
                        "'in': expected region name after 'in'",
                    );
                    return self.dbg_exit("parse_section", false);
                }
                // Synthesize a RegionRef node to record the binding.
                let ref_node = TokenInfo {
                    tok: LexToken::RegionRef,
                    loc: tinfo.loc.clone(),
                    val: tinfo.val,
                };
                self.tv.skip(); // consume region name token
                let ref_nid = self.arena.new_node(ref_node);
                sec_nid.append(ref_nid, &mut self.arena);
            }

            // After the name (and optional region binding), expect an open brace.
            // Remember the location of the opening brace to help with
            // user missing brace errors.
            let brace_toknum = self.tv.get_index();
            if self.expect_leaf(
                diags,
                sec_nid,
                LexToken::OpenBrace,
                "ERR_2",
                "Expected { after identifier",
            ) {
                result = self.parse_section_contents(sec_nid, diags, brace_toknum);
            }
        }
        self.dbg_exit("parse_section", result)
    }

    /// Returns `true` if `tok` is one of the tokens that a section body dispatches
    /// to `parse_expr`.  Centralized here so adding a new write instruction (e.g.
    /// a future `Wr128`) only requires updating this one predicate.
    fn is_section_expr_tok(tok: LexToken) -> bool {
        matches!(
            tok,
            LexToken::Wrf
                | LexToken::Wr8
                | LexToken::Wr16
                | LexToken::Wr24
                | LexToken::Wr32
                | LexToken::Wr40
                | LexToken::Wr48
                | LexToken::Wr56
                | LexToken::Wr64
                | LexToken::Wrbe8
                | LexToken::Wrbe16
                | LexToken::Wrbe24
                | LexToken::Wrbe32
                | LexToken::Wrbe40
                | LexToken::Wrbe48
                | LexToken::Wrbe56
                | LexToken::Wrbe64
                | LexToken::Wrs
                | LexToken::Assert
                | LexToken::Align
                | LexToken::PadSecOffset
                | LexToken::PadAddrOffset
                | LexToken::SetAddr
                | LexToken::PadFileOffset
                | LexToken::Print
                | LexToken::Trace
        )
    }

    /// Try to parse one section-body statement starting at the current token.
    ///
    /// Handles the three dispatch cases shared by `parse_section_contents` and
    /// `parse_if_body_r` (Section context):
    ///   - `Label`                        → `parse_label`
    ///   - `Wr`                           → `parse_wr`
    ///   - `is_section_expr_tok` tokens   → `parse_expr`
    ///
    /// Returns `Some(result)` if the token was handled, `None` if it is not a
    /// recognized section-level token (caller is responsible for the error).
    fn try_parse_section_stmt(&mut self, parent: NodeId, diags: &mut Diags) -> Option<bool> {
        let tok = self.tv.peek().tok;
        match tok {
            LexToken::Label => Some(self.parse_label(parent, diags)),
            LexToken::Wr => Some(self.parse_wr(parent, diags)),
            tok if Self::is_section_expr_tok(tok) => Some(self.parse_expr(parent, diags)),
            _ => None,
        }
    }

    /// Parses the body of a section, appending statement nodes directly to the
    /// parent section node.  Loops until a `}` is found or tokens are exhausted.
    /// Each iteration dispatches via `try_parse_section_stmt`; unrecognized tokens
    /// produce a diagnostic and are skipped to the next `;` to allow recovery.
    ///
    /// ```text
    /// wr8 1+2; assert x; }
    ///
    ///   section              <- parent node (owned by parse_section)
    ///   ...
    ///   ├── <statement>      <- one node per statement in the section body
    ///   ├── <statement>
    ///   └── }                <- close-brace leaf, signals end of body
    /// ```
    fn parse_section_contents(
        &mut self,
        parent: NodeId,
        diags: &mut Diags,
        brace_tok_num: usize,
    ) -> bool {
        self.dbg_enter("parse_section_contents");
        let mut result = true; // todo fixme

        let mut tok_idx_old = 0;
        loop {
            debug_peek!("Ast::parse_section_contents", self.tv);
            let tinfo = self.tv.peek();
            if tinfo.tok == LexToken::EOF {
                break;
            }
            let tok_idx_new = self.tv.get_index();
            if tok_idx_old == tok_idx_new {
                // In some error cases, such as a missing closing brace, parsing
                // can get stuck without advancing the token pointer.  For
                // example, this problem occurs because an error occurs at the
                // very start of a new expression.  The advance_past_semicolon
                // function won't move us forward since we're already past a
                // semicolon and at the start of a new statement. As a simple
                // solution, detect that we're not making forward progress and
                // force the token number forward.
                self.tv.skip();
                debug!("parse_section_contents: Forcing forward progress.");
                continue;
            }
            tok_idx_old = tok_idx_new;
            // When we find a close brace, we're done with section content
            if tinfo.tok == LexToken::CloseBrace {
                self.parse_leaf(parent);
                return self.dbg_exit("parse_section_contents", result);
            }

            // Stay in the section even after errors to give the user
            // more than one error at a time
            let parse_ok = if tinfo.tok == LexToken::If {
                self.parse_if_r(parent, diags, ParseIfContext::Section)
            } else if let Some(result) = self.try_parse_section_stmt(parent, diags) {
                result
            } else {
                self.err_invalid_expression(diags, "ERR_3");
                false
            };

            if !parse_ok {
                debug!(
                    "Ast::parse_section_contents: skipping to next ; starting from {}",
                    self.tv.get_index()
                );
                // Consume the bad token and skip forward
                self.advance_past_semicolon();
                result = false;
            }
        }

        // If we got here, we ran out of tokens before finding the close brace.
        self.err_no_close_brace(diags, brace_tok_num);
        self.dbg_exit("parse_section_contents", false)
    }

    /// Parses a `wr` statement that copies a named section into the output
    /// or evaluates an extension call.
    ///
    /// ```text
    /// wr <name>;
    ///
    ///   wr               <- root node for a section-write statement
    ///   └── <Identifier> <- name of the section to be written
    ///
    /// or
    ///
    /// wr <namespace::extension_name>(<args>);
    ///
    ///   wr               <- root node for an extension call
    ///   └── <namespace::extension_name>(<args>) <- extension call
    /// ```
    fn parse_wr(&mut self, parent_nid: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_wr");
        let mut result = false;

        // Add the wr keyword as a child of the parent and advance
        let wr_nid = self.add_to_parent_and_advance(parent_nid);

        // Next, an expression is expected
        if self.expect_expr(wr_nid, diags) {
            result = self.expect_semi(diags, wr_nid);
        }
        self.dbg_exit("parse_wr", result)
    }

    /// Returns the (lhs, rhs) binding power for any infix token.
    /// Higher numbers are stronger binding. Returns None if the
    /// token is not a valid infix operator.
    fn get_infix_binding_power(tok: LexToken) -> Option<(u8, u8)> {
        match tok {
            LexToken::Percent | LexToken::FSlash | LexToken::Asterisk => Some((19, 20)),
            LexToken::Minus | LexToken::Plus => Some((17, 18)),
            LexToken::DoubleLess | LexToken::DoubleGreater => Some((15, 16)),
            LexToken::Ampersand => Some((13, 14)),
            LexToken::Pipe => Some((11, 12)),
            LexToken::DoubleEq
            | LexToken::NEq
            | LexToken::LEq
            | LexToken::GEq
            | LexToken::Lt
            | LexToken::Gt => Some((9, 10)),
            LexToken::DoubleAmpersand => Some((7, 8)),
            LexToken::DoublePipe => Some((5, 6)),
            _ => None,
        }
    }

    /// Parses the arguments of a function or extension invocation.
    /// The caller must have already consumed the opening parenthesis `(`.
    /// This function consumes the closing parenthesis `)`.
    fn parse_function_args(&mut self, parent_nid: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_function_args");
        let mut saw_named = false;
        let mut saw_positional = false;

        loop {
            debug_peek!("Ast::parse_function_args call", self.tv);
            let check_tinfo = self.tv.peek();
            if check_tinfo.tok == LexToken::EOF {
                self.err_no_input(diags);
                return self.dbg_exit("parse_function_args", false);
            }

            // A trailing close parenthesis indicates the end of the argument list.
            if check_tinfo.tok == LexToken::CloseParen {
                self.tv.skip(); // consume ')'
                break;
            }

            // Named arg: Identifier immediately followed by Eq (not DoubleEq).
            let idx = self.tv.get_index();
            let is_named_arg = self.tv.peek().tok == LexToken::Identifier
                && self.tv.get(idx + 1).tok == LexToken::Eq;

            if is_named_arg {
                debug!("Ast::parse_function_args: Detected named argument syntax");
                saw_named = true;
                // Borrow name and loc from the identifier token, then advance.
                let param_name = self.tv.peek().val;
                let param_loc = self.tv.peek().loc.clone();
                self.tv.skip(); // consume Identifier
                self.tv.skip(); // consume Eq

                // Synthesize a NamedArg token and create a node for it.
                let synthetic = TokenInfo {
                    tok: LexToken::NamedArg,
                    loc: param_loc.clone(),
                    val: param_name,
                };
                let named_nid = self.arena.new_node(synthetic);
                parent_nid.append(named_nid, &mut self.arena);

                // Parse the RHS expression as the sole child of the NamedArg node.
                let mut rhs_opt = None;
                if !self.parse_pratt(0, &mut rhs_opt, diags) {
                    return self.dbg_exit("parse_function_args", false);
                }
                if let Some(rhs_nid) = rhs_opt {
                    // The RHS of a named argument becomes a child of the NamedArg node in the AST.
                    named_nid.append(rhs_nid, &mut self.arena);
                } else {
                    diags.err1(
                        "ERR_36",
                        "Expected expression after '=' in named argument",
                        param_loc,
                    );
                    return self.dbg_exit("parse_function_args", false);
                }
            } else {
                saw_positional = true;
                // Positional argument: parse the expression directly.
                let mut arg_opt = None;
                if !self.parse_pratt(0, &mut arg_opt, diags) {
                    return self.dbg_exit("parse_function_args", false);
                }
                if let Some(arg_nid) = arg_opt {
                    parent_nid.append(arg_nid, &mut self.arena);
                }
            }

            // Reject mixed positional and named arguments.
            if saw_named && saw_positional {
                diags.err1(
                    "ERR_35",
                    "Cannot mix positional and named arguments in an extension call",
                    self.tv.peek().loc.clone(),
                );
                return self.dbg_exit("parse_function_args", false);
            }

            // Arguments must be separated by commas or terminated by a close parenthesis.
            let delim_tinfo = self.tv.peek();
            if delim_tinfo.tok == LexToken::EOF {
                self.err_no_input(diags);
                return self.dbg_exit("parse_function_args", false);
            }

            let delim_tok = delim_tinfo.tok;
            if delim_tok == LexToken::Comma {
                self.tv.skip(); // consume ','
            } else if delim_tok == LexToken::CloseParen {
                self.tv.skip(); // consume ')'
                break;
            } else {
                diags.err1(
                    "ERR_33",
                    "Expected ',' or ')' in function call",
                    delim_tinfo.span(),
                );
                return self.dbg_exit("parse_function_args", false);
            }
        }

        self.dbg_exit("parse_function_args", true)
    }

    /// Parses an expression with correct operator precedence using a Pratt
    /// (precedence-climbing) algorithm.  Returns the root `NodeId` of the
    /// sub-tree via `top`, or `None` if the expression is empty.  On success
    /// the terminal `;`, `,`, or `)` remains as the next unprocessed token.
    /// See <https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html>.
    ///
    /// ```text
    /// 1 + 2 * 3
    ///
    ///   +                <- root is the lowest-precedence operator
    ///   ├── 1            <- left atom
    ///   └── *            <- higher-precedence sub-expression
    ///       ├── 2
    ///       └── 3
    ///
    /// sizeof(<name>)
    ///
    ///   sizeof
    ///   └── <Identifier> <- mandatory section or label name
    ///
    /// abs([<name>])
    ///
    ///   abs
    ///   └── [<Identifier>] <- optional section or label name
    /// ```
    fn parse_pratt(&mut self, min_bp: u8, top: &mut Option<NodeId>, diags: &mut Diags) -> bool {
        debug!("Ast::parse_pratt: ENTER, Min BP = {}", min_bp);
        debug_peek!("Ast::parse_pratt", self.tv);

        let Some(_guard) = DepthGuard::enter(MAX_RECURSION_DEPTH) else {
            let tinfo = self.tv.peek();
            diags.err1(
                "ERR_38",
                &format!("Expression nesting depth exceeds maximum ({MAX_RECURSION_DEPTH})."),
                tinfo.loc.clone(),
            );
            return false;
        };

        let lhs_tinfo = self.tv.peek();

        *top = None; // Initialize our root node.

        match lhs_tinfo.tok {
            LexToken::EOF => {
                self.err_no_input(diags);
                return self.dbg_exit_pratt("parse_pratt", &None, false);
            }

            // Finding a close paren or a semi-colon terminates an expression.
            LexToken::CloseParen | LexToken::Semicolon => {
                /* top will be None */
                *top = None;
            }

            // This open paren is precedence control in an expression, e.g. (1+2)*3.
            // This is not an open paren associated with a built-in function.
            LexToken::OpenParen => {
                // move past the open paren without storing in the AST.
                self.tv.skip();
                // lhs is everything inside parentheses.
                if !self.parse_pratt(0, top, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                // Open paren must have a matching close paren.
                if !self.expect_token_no_add(LexToken::CloseParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
            }

            // These simple atoms end up as leaf nodes in the AST
            LexToken::QuotedString | LexToken::Integer | LexToken::I64 | LexToken::U64 => {
                *top = Some(self.arena.new_node(self.tv.peek().clone()));
                self.tv.skip();
            }

            // A namespace component like `custom::` signals the start of a namespaced path.
            // This token must be immediately followed by a trailing identifier. If an open parenthesis
            // `(` follows the identifier, the parser aggregates the tokens into a generic
            // function invocation (e.g., `custom::foo(arg1, arg2)`).
            LexToken::Namespace => {
                let ns_nid = self.arena.new_node(self.tv.peek().clone());
                *top = Some(ns_nid);
                self.tv.skip();

                // A namespace prefix must immediately be followed by an identifier.
                debug_peek!("Ast::parse_pratt namespace", self.tv);
                let next_tinfo = self.tv.peek();
                if next_tinfo.tok == LexToken::EOF {
                    self.err_no_input(diags);
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }

                // Add the trailing identifier as the first child of the namespace node.
                if next_tinfo.tok == LexToken::Identifier {
                    let id_nid = self.arena.new_node(self.tv.peek().clone());
                    ns_nid.append(id_nid, &mut self.arena);
                    self.tv.skip();
                } else {
                    diags.err1(
                        "ERR_34",
                        "Expected identifier after namespace",
                        next_tinfo.span(),
                    );
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }

                // If an open parenthesis follows, we parse this as a function invocation.
                let after_tinfo = self.tv.peek();
                if after_tinfo.tok == LexToken::OpenParen {
                    self.tv.skip(); // consume '('
                    if !self.parse_function_args(ns_nid, diags) {
                        return self.dbg_exit_pratt("parse_pratt", &None, false);
                    }
                }
            }

            // Identifiers are usually scalar variables or section names.
            // However, if an identifier is immediately followed by an open parenthesis `(`,
            // the parser actively eats tokens looking for the trailing close parenthesis `)`
            // to construct a generic function invocation, e.g., `foo(arg1, arg2)`. At this
            // AST stage, we parse all arguments without verifying function support. That
            // validation happens in later phases.
            LexToken::Identifier => {
                let id_nid = self.arena.new_node(self.tv.peek().clone());
                *top = Some(id_nid);
                self.tv.skip();

                // If an open parenthesis follows, we parse this as a function invocation.
                if self.tv.peek().tok == LexToken::OpenParen {
                    self.tv.skip(); // consume '('
                    if !self.parse_function_args(id_nid, diags) {
                        return self.dbg_exit_pratt("parse_pratt", &None, false);
                    }
                }
            }

            // Built-in functions with an optional identifier inside parens
            // ( [optional identifier] )
            LexToken::Addr | LexToken::AddrOffset | LexToken::SecOffset | LexToken::FileOffset => {
                // Create the node for the function and move past
                *top = Some(self.arena.new_node(self.tv.peek().clone()));
                self.tv.skip();

                if !self.expect_token_no_add(LexToken::OpenParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                if !self.optional_token(&[LexToken::Identifier], diags, top.unwrap()) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                if !self.expect_token_no_add(LexToken::CloseParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
            }

            // Build-in functions with a mandatory identifier inside parens
            // ( <identifier> )
            LexToken::Sizeof => {
                *top = Some(self.arena.new_node(self.tv.peek().clone()));
                self.tv.skip();

                if !self.expect_token_no_add(LexToken::OpenParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                let mut arg_opt = None;
                if !self.parse_pratt(0, &mut arg_opt, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }

                // Check the children to determine if this sizeof() is valid.
                let is_valid = if let Some(arg_nid) = arg_opt {
                    let arg_tinfo = self.get_tinfo(arg_nid);
                    if arg_tinfo.tok == LexToken::Identifier {
                        // An identifier has children if parsed as a function call,
                        // which is an error.
                        !self.has_children(arg_nid)
                    } else if arg_tinfo.tok == LexToken::Namespace {
                        // A namespace has exactly one child (the trailing identifier) if called
                        // correctly as just an extension name, e.g. `sizeof(foo::bar)`.
                        // If the namespace has > 1 child, the user tried to pass arguments
                        // to sizeof(), which is an error.
                        arg_nid.children(&self.arena).count() == 1
                    } else {
                        false
                    }
                } else {
                    false
                };

                if !is_valid {
                    let err_span = arg_opt.map_or(self.get_tinfo(top.unwrap()).span(), |nid| {
                        self.get_tinfo(nid).span()
                    });
                    diags.err1("ERR_35", "sizeof() accepts only a section name or an extension identifier without arguments", err_span);
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }

                top.unwrap().append(arg_opt.unwrap(), &mut self.arena);
                if !self.expect_token_no_add(LexToken::CloseParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
            }

            // obj_align/obj_lma/obj_vma: mandatory single obj identifier in parens
            LexToken::ObjAlign | LexToken::ObjLma | LexToken::ObjVma => {
                *top = Some(self.arena.new_node(self.tv.peek().clone()));
                self.tv.skip();
                if !self.expect_token_no_add(LexToken::OpenParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                let mut arg_opt = None;
                if !self.parse_pratt(0, &mut arg_opt, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                let valid = arg_opt.is_some_and(|nid| {
                    let t = self.get_tinfo(nid);
                    t.tok == LexToken::Identifier && !self.has_children(nid)
                });
                if !valid {
                    let err_span = arg_opt.map_or(self.get_tinfo(top.unwrap()).span(), |nid| {
                        self.get_tinfo(nid).span()
                    });
                    diags.err1(
                        "ERR_73",
                        "obj_align/obj_lma/obj_vma requires exactly one obj name",
                        err_span,
                    );
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                top.unwrap().append(arg_opt.unwrap(), &mut self.arena);
                if !self.expect_token_no_add(LexToken::CloseParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
            }

            // Built-in functions with a non-optional expression inside parens
            // ( <expr> )
            LexToken::ToI64 | LexToken::ToU64 => {
                *top = Some(self.arena.new_node(self.tv.peek().clone()));
                self.tv.skip();

                if !self.expect_token_no_add(LexToken::OpenParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                if !self.expect_expr(top.unwrap(), diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
                if !self.expect_token_no_add(LexToken::CloseParen, diags) {
                    return self.dbg_exit_pratt("parse_pratt", &None, false);
                }
            }

            // Built-in variable atoms — no parentheses, no arguments.
            LexToken::BuiltinOutputSize
            | LexToken::BuiltinOutputAddr
            | LexToken::BuiltinVersionString
            | LexToken::BuiltinVersionMajor
            | LexToken::BuiltinVersionMinor
            | LexToken::BuiltinVersionPatch => {
                *top = Some(self.arena.new_node(self.tv.peek().clone()));
                self.tv.skip();
            }

            _ => {
                let msg = format!("Invalid expression operand '{}'", lhs_tinfo.val);
                diags.err1("ERR_17", &msg, lhs_tinfo.span());
                return self.dbg_exit_pratt("parse_pratt", &None, false);
            }
        };

        // Clean exit if this expression had no more tokens
        if (*top).is_none() {
            return self.dbg_exit_pratt("parse_pratt", &None, true);
        }

        // Keep processing for the remaining right hand side of the expression.
        loop {
            // We expect an operation such as add, a semicolon, etc. or the end of input.
            let op_tinfo = self.tv.peek();
            if op_tinfo.tok == LexToken::EOF {
                break; // end of input.
            }

            // Filter disallowed operations.
            let tok = op_tinfo.tok;

            // Comma, close paren, semicolon, open brace, and else are terminating
            // conditions because some upper layer is specifically looking for them.
            if matches!(
                tok,
                LexToken::Comma
                    | LexToken::CloseParen
                    | LexToken::Semicolon
                    | LexToken::OpenBrace
                    | LexToken::Else
            ) {
                break;
            }

            let Some((lbp, rbp)) = Ast::get_infix_binding_power(tok) else {
                let msg = format!("Invalid operation '{}'", op_tinfo.val);
                diags.err1("ERR_8", &msg, op_tinfo.span());
                return self.dbg_exit_pratt("parse_pratt", &None, false);
            };

            debug!(
                "Ast::parse_pratt: operation '{}' with (lbp,rbp) = ({},{})",
                op_tinfo.val, lbp, rbp
            );

            // A decrease in operator precedence ends the iteration.
            if lbp < min_bp {
                break;
            }

            let op_nid = self.arena.new_node(self.tv.peek().clone());
            self.tv.skip();

            // Attach the old top as a child of the operation,
            // then update the new top node
            op_nid.append(top.unwrap(), &mut self.arena);

            // The operation is the new left-hand-side from our caller's point of view
            *top = Some(op_nid);

            // Recurse into the right hand side of the operation, if any
            let mut rhs_opt = None;
            if !self.parse_pratt(rbp, &mut rhs_opt, diags) {
                return self.dbg_exit_pratt("parse_pratt", &None, false);
            }

            if let Some(rhs_nid) = rhs_opt {
                op_nid.append(rhs_nid, &mut self.arena);
            } else {
                // RHS is none
                break;
            }
        }

        self.dbg_exit_pratt("parse_pratt", top, true)
    }

    /// Parses a keyword statement whose operands are one or more comma-separated
    /// expressions.  Handles `wr8`..`wr64`, `wrs`, `wrf`, `assert`, `align`,
    /// `set*`, and `print`.
    ///
    /// ```text
    /// print <expr> [, <expr>] ;
    ///
    ///   <keyword>        <- root node (e.g. print, wr8, assert)
    ///   ├── <expr>       <- first expression (sub-tree from parse_pratt)
    ///   └── [<expr>...]  <- additional comma-separated expressions, if any
    /// ```
    fn parse_expr(&mut self, parent: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_multi_expr");
        let mut result = true;
        // Add the print keyword as a child of the parent
        let print_nid = self.add_to_parent_and_advance(parent);

        let mut expr_opt = None;

        // Loop until we run out of comma separated expressions.
        // After each return from parse_pratt, we should be pointing at a comma.
        loop {
            result &= self.parse_pratt(0, &mut expr_opt, diags);
            if !result {
                break; // error occurred
            }
            if let Some(expr_nid) = expr_opt {
                print_nid.append(expr_nid, &mut self.arena);

                // wrf takes exactly one argument; a comma after the first is an error.
                if self.get_tinfo(print_nid).tok == LexToken::Wrf
                    && self.tv.peek().tok == LexToken::Comma
                {
                    diags.err1(
                        "ERR_37",
                        "'wrf' takes exactly one argument",
                        self.get_tinfo(print_nid).span(),
                    );
                    self.advance_past_semicolon();
                    result = false;
                    break;
                }

                // Omit the comma from the AST to reduce clutter.
                if self.tv.peek().tok == LexToken::Comma {
                    self.tv.skip();
                    continue;
                }

                // If not a comma, then we expect semi.
                result &= self.expect_semi(diags, print_nid);
                break;
            } else {
                // the print statement ended in some unusual way, e.g. trailing comma.
                // fuzz test found this with print 1,;
                let msg = "Statement ended unexpectedly";
                let tinfo = self.get_tinfo(print_nid);
                diags.err1("ERR_19", msg, tinfo.span());
                result = false;
                break;
            }
        }

        self.dbg_exit("parse_multi_expr", result)
    }

    /// Parses a `label` statement.  Labels mark a named address within a section
    /// body and produce no output bytes; they exist solely so other expressions
    /// can reference the address via `addr()`, `addr_offset()`, or `sec_offset()`.
    ///
    /// ```text
    /// label <name>;
    ///
    ///   label            <- leaf node, value holds the label name
    /// ```
    fn parse_label(&mut self, parent: NodeId, _diags: &mut Diags) -> bool {
        // Not much to do since labels just mark a place but
        // cause no actions.
        self.dbg_enter("parse_label");
        self.add_to_parent_and_advance(parent);
        self.dbg_exit("parse_assert", true)
    }

    /// Parses the top-level `output` statement that designates which section
    /// is written to the output file and an optional absolute base address.
    ///
    /// ```text
    /// output <name>;
    ///
    ///   output               <- root node for the output declaration
    ///   └── <Identifier>     <- name of the section to emit
    /// ```
    fn parse_output(&mut self, parent: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_output");
        let mut result = false;
        // Add the section keyword as a child of the parent and advance
        let output_nid = self.add_to_parent_and_advance(parent);

        // After 'output' a section identifier is expected.  expect_name_leaf also
        // accepts keyword tokens so AstDb can emit the specific reserved-name error.
        if self.expect_name_leaf(
            diags,
            output_nid,
            "ERR_6",
            "Expected a section name after output",
        ) {
            // Reject old syntax: output <name> <addr>;
            // The address argument was removed; use set_addr inside the section.
            let tinfo = self.tv.peek();
            if matches!(
                tinfo.tok,
                LexToken::U64 | LexToken::Integer | LexToken::Identifier
            ) {
                let msg = format!(
                    "output no longer accepts a starting address ('{}'); use set_addr inside the section instead",
                    tinfo.val
                );
                diags.err1("ERR_50", &msg, tinfo.span());
                // Consume the address token and the trailing semicolon so that
                // the parser does not emit cascading errors for those tokens.
                self.tv.skip();
                if self.tv.peek().tok == LexToken::Semicolon {
                    self.tv.skip();
                }
                return self.dbg_exit("parse_output", false);
            }

            // finally a semicolon
            result = self.expect_semi(diags, output_nid);
        }

        self.dbg_exit("parse_output", result)
    }

    /// Parses a `const` declaration and attaches it to the AST.
    ///
    /// ```text
    /// const <name> = <expr>;
    /// const <name>;  // deferred assignment
    ///
    ///   const            <- root node for a const declaration
    ///   ├── <Identifier> <- constant name
    ///   ├── =            <- syntactic separator, not an operation
    ///   └── <expr>       <- right-hand side (literal or expression)
    /// ```
    ///
    /// The `=` node is retained as a child to preserve source location
    /// information but carries no semantic meaning in later pipeline stages.
    fn parse_const(&mut self, parent: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_const");
        let mut result = false;
        // Add the const keyword as a child of the parent and advance
        let const_nid = self.add_to_parent_and_advance(parent);

        // After 'const' an identifier is expected.  expect_name_leaf also accepts
        // keyword tokens so AstDb can emit the specific reserved-identifier error.
        if self.expect_name_leaf(
            diags,
            const_nid,
            "ERR_7",
            "Expected an identifier after 'const'",
        ) {
            // After the identifier: either '=' (full definition) or ';' (declare-only).
            let tinfo = self.tv.peek();
            if tinfo.tok == LexToken::EOF {
                self.err_no_input(diags);
            } else if tinfo.tok == LexToken::Eq {
                // Full definition: const NAME = expr;
                if self.expect_token(LexToken::Eq, diags, const_nid)
                    && self.expect_expr(const_nid, diags)
                {
                    result = self.expect_semi(diags, const_nid);
                }
            } else if tinfo.tok == LexToken::Semicolon {
                // Declare-only: const NAME;  (value assigned later in an if/else body)
                result = self.expect_semi(diags, const_nid);
            } else {
                self.err_expected_after(
                    diags,
                    "ERR_45",
                    "Expected '=' or ';' after const identifier",
                );
            }
        }
        self.dbg_exit("parse_const", result)
    }

    /// Recursively parses an `if/else` statement and attaches it to the AST.
    ///
    /// ```text
    /// if <expr> { <body> } [else { <body> } | else if ...]
    ///
    ///   if                  <- root node
    ///   ├── <condition>     <- pratt expression
    ///   ├── {
    ///   ├── [then_stmts...] <- see parse_if_body
    ///   ├── }
    ///   [├── else
    ///    ├── {              <- or nested if node for `else if`
    ///    ├── [else_stmts...]
    ///    └── }]
    /// ```
    fn parse_if_r(&mut self, parent: NodeId, diags: &mut Diags, ctx: ParseIfContext) -> bool {
        self.dbg_enter("parse_if");

        let Some(_guard) = DepthGuard::enter(MAX_RECURSION_DEPTH) else {
            let tinfo = self.tv.peek();
            diags.err1(
                "ERR_39",
                &format!("if/else nesting depth exceeds maximum ({MAX_RECURSION_DEPTH})."),
                tinfo.loc.clone(),
            );
            return false;
        };

        // Consume 'if' and create root node
        let if_nid = self.add_to_parent_and_advance(parent);

        // Parse condition expression
        if !self.expect_expr(if_nid, diags) {
            return self.dbg_exit("parse_if", false);
        }

        // Expect opening brace for then-body
        let brace_toknum = self.tv.get_index();
        if !self.expect_leaf(
            diags,
            if_nid,
            LexToken::OpenBrace,
            "ERR_46",
            "Expected '{' after if condition",
        ) {
            return self.dbg_exit("parse_if", false);
        }
        if !self.parse_if_body_r(if_nid, diags, brace_toknum, ctx) {
            return self.dbg_exit("parse_if", false);
        }

        // Check for optional else clause
        let tinfo = self.tv.peek();
        let result = if tinfo.tok == LexToken::EOF {
            self.err_no_input(diags);
            false
        } else if tinfo.tok == LexToken::Else {
            self.add_to_parent_and_advance(if_nid); // consume 'else', add as child
            let next = self.tv.peek();
            if next.tok == LexToken::If {
                // else if: parse nested if directly (no brace wrapper)
                self.parse_if_r(if_nid, diags, ctx)
            } else if next.tok == LexToken::OpenBrace {
                let else_brace = self.tv.get_index();
                self.add_to_parent_and_advance(if_nid); // consume '{'
                self.parse_if_body_r(if_nid, diags, else_brace, ctx)
            } else if next.tok == LexToken::EOF {
                self.err_no_input(diags);
                false
            } else {
                self.err_expected_after(diags, "ERR_47", "Expected '{' or 'if' after 'else'");
                false
            }
        } else {
            true // no else clause
        };

        self.dbg_exit("parse_if", result)
    }

    /// Recursively (by way of parse_if_r) parses the body of an if/else statement.
    ///
    /// In `TopLevel` context, only const-compatible statements are allowed:
    /// bare assignment (`IDENT = expr;`), `print`, `assert`, and nested `if/else`.
    ///
    /// In `Section` context, all section-level statements are also allowed:
    /// `wr`, `wr8`–`wr64`, `wrs`, `wrf`, `align`, `set_*`, `label:`, and nested `if/else`.
    fn parse_if_body_r(
        &mut self,
        parent: NodeId,
        diags: &mut Diags,
        brace_tok_num: usize,
        ctx: ParseIfContext,
    ) -> bool {
        self.dbg_enter("parse_if_body");
        let mut result = true;
        let mut tok_idx_old = 0;

        loop {
            let tinfo = self.tv.peek();
            if tinfo.tok == LexToken::EOF {
                break;
            }
            let tok_idx_new = self.tv.get_index();
            if tok_idx_old == tok_idx_new {
                self.tv.skip();
                continue;
            }
            tok_idx_old = tok_idx_new;

            if tinfo.tok == LexToken::CloseBrace {
                self.parse_leaf(parent); // add '}' as child
                return self.dbg_exit("parse_if_body", result);
            }

            let parse_ok = match tinfo.tok {
                // Const-compatible statements (allowed in both TopLevel and Section)
                LexToken::Identifier => self.parse_deferred_assign(parent, diags),
                LexToken::Print | LexToken::Trace | LexToken::Assert => {
                    self.parse_expr(parent, diags)
                }
                LexToken::If => self.parse_if_r(parent, diags, ctx),
                // Section-level statements: delegate to the shared dispatcher.
                // try_parse_section_stmt returns None for unrecognized tokens.
                _ => {
                    // Snapshot before mutable borrows below.
                    let err_val = tinfo.val.to_string();
                    let err_span = tinfo.span();
                    let tok = tinfo.tok;
                    let maybe = match ctx {
                        ParseIfContext::Section => self.try_parse_section_stmt(parent, diags),
                        ParseIfContext::TopLevel if tok == LexToken::Section => {
                            Some(self.parse_section(parent, diags))
                        }
                        _ => None,
                    };
                    maybe.unwrap_or_else(|| {
                        let msg = format!("'{}' is not allowed inside an if/else body", err_val);
                        diags.err1("ERR_48", &msg, err_span);
                        false
                    })
                }
            };

            if !parse_ok {
                self.advance_past_semicolon();
                result = false;
            }
        }

        self.err_no_close_brace(diags, brace_tok_num);
        self.dbg_exit("parse_if_body", false)
    }

    /// Parses a deferred assignment statement `IDENT = expr ;` inside an if/else body.
    ///
    /// ```text
    ///   =                  <- root node (the assignment operator)
    ///   ├── <Identifier>   <- LHS name
    ///   └── <expr>         <- RHS value expression
    /// ```
    fn parse_deferred_assign(&mut self, parent: NodeId, diags: &mut Diags) -> bool {
        self.dbg_enter("parse_deferred_assign");

        // Confirm the token after the identifier is '=' (not '==', which would be a comparison).
        // Short-circuit guarantees idx+1 is in bounds: if current tok is Identifier, idx < eof_idx.
        let idx = self.tv.get_index();
        let next_tinfo = self.tv.get(idx + 1);
        if next_tinfo.tok != LexToken::Eq {
            let found = if next_tinfo.tok == LexToken::EOF {
                "<end of input>"
            } else {
                next_tinfo.val
            };
            let msg = format!(
                "Expected '=' after identifier in deferred const assignment, found '{}'",
                found
            );
            diags.err1("ERR_49", &msg, self.tv.peek().span());
            // We don't want the arbitrary chars in an unknown identifier to
            // propogate any further, so eat the identifier here.
            self.tv.skip();
            return self.dbg_exit("parse_deferred_assign", false);
        }

        // Create the identifier node without attaching it to a parent yet.
        let ident_nid = self.arena.new_node(self.tv.peek().clone());
        self.tv.skip();

        // Create the Eq node as the statement root (attached to parent).
        let eq_nid = self.add_to_parent_and_advance(parent);

        // Attach identifier as the first child of the Eq node.
        eq_nid.append(ident_nid, &mut self.arena);

        // Parse RHS expression as the second child.
        if !self.expect_expr(eq_nid, diags) {
            return self.dbg_exit("parse_deferred_assign", false);
        }

        let result = self.expect_semi(diags, eq_nid);
        self.dbg_exit("parse_deferred_assign", result)
    }

    /// Adds the current token as a child of the parent and advances
    /// the token index.  The current token MUST BE VALID!
    fn parse_leaf(&mut self, parent: NodeId) {
        let nid = self.arena.new_node(self.tv.peek().clone());
        parent.append(nid, &mut self.arena);
        self.tv.skip();
    }

    pub fn get_tinfo(&self, nid: NodeId) -> &TokenInfo<'toks> {
        self.arena[nid].get()
    }

    /// Returns the root NodeId of the AST.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Returns a mutable reference to the underlying indextree arena.
    /// Callers can use any indextree `NodeId` operation that requires `&mut Arena`.
    pub fn arena_mut(&mut self) -> &mut Arena<TokenInfo<'toks>> {
        &mut self.arena
    }

    const DOT_DEFAULT_FILL: &'static str = "#F2F2F2";
    const DOT_DEFAULT_EDGE: &'static str = "#808080";
    const DOT_DEFAULT_PEN: &'static str = "#808080";

    fn dump_r(&self, nid: NodeId, depth: usize, file: &mut File) -> anyhow::Result<()> {
        debug!(
            "AST: {}: {}{}",
            nid,
            " ".repeat(depth * 4),
            self.get_tinfo(nid).val
        );
        let tinfo = self.get_tinfo(nid);

        let (label, color) = match tinfo.tok {
            LexToken::QuotedString => {
                if tinfo.val.len() <= 8 {
                    (
                        tinfo
                            .val
                            .strip_prefix('\"')
                            .unwrap()
                            .strip_suffix('\"')
                            .unwrap(),
                        Ast::DOT_DEFAULT_FILL,
                    )
                } else {
                    ("<string>", Ast::DOT_DEFAULT_FILL)
                }
            }
            LexToken::Unknown => ("<unknown>", "red"),
            _ => (tinfo.val, Ast::DOT_DEFAULT_FILL),
        };

        file.write(format!("{} [label=\"{}\",fillcolor=\"{}\"]\n", nid, label, color).as_bytes())
            .context("ast.dot write failed")?;
        let children = nid.children(&self.arena);
        for child_nid in children {
            file.write(format!("{} -> {}\n", nid, child_nid).as_bytes())
                .context("ast.dot write failed")?;
            self.dump_r(child_nid, depth + 1, file)?;
        }
        Ok(())
    }

    /// Recursively dumps the AST to file ast.dot in Graphviz dot format. View
    /// the graph with `dot -Tpng ast.dot -o ast.png` or use an online Graphviz
    /// viewer.
    pub fn dump(&self, fname: &str) -> anyhow::Result<()> {
        debug!("");
        let mut file = File::create(fname)
            .context(format!("Error attempting to create debug file '{}'", fname))?;
        file.write(b"digraph {\n").context("ast.dot write failed")?;
        file.write(
            format!(
                "node [style=filled,fillcolor=\"{}\",color=\"{}\"]\n",
                Ast::DOT_DEFAULT_FILL,
                Ast::DOT_DEFAULT_PEN
            )
            .as_bytes(),
        )
        .context("ast.dot write failed")?;
        file.write(format!("edge [color=\"{}\"]\n", Ast::DOT_DEFAULT_EDGE).as_bytes())
            .context("ast.dot write failed")?;

        file.write(format!("{} [label=\"root\"]\n", self.root).as_bytes())
            .context("ast.dot write failed")?;
        let children = self.root.children(&self.arena);
        for child_nid in children {
            file.write(format!("{} -> {}\n", self.root, child_nid).as_bytes())
                .context("ast.dot write failed")?;
            self.dump_r(child_nid, 0, &mut file)?;
        }

        file.write(b"}\n").context("ast.dot write failed")?;
        debug!("");
        Ok(())
    }
}
