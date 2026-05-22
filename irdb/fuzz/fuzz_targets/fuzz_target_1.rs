#![no_main]
// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use libfuzzer_sys::fuzz_target;
use ast::{Ast,AstDb};
use std::io::Write;
use diags::Diags;
use lineardb::LinearDb;

// The fuzzer calls this function repeatedly
fuzz_target!(|data: &[u8]| {
    if let Ok(str_in) = std::str::from_utf8(data) {
        // Set the verbosity to 0 to avoid console error
        // messages during the test.
        let mut diags = Diags::new("fuzz_target_1",str_in, 0, false);
        if let Ok(ast) = Ast::new(str_in, &mut diags) {
            if let Ok(ast_db) = AstDb::new(&mut diags, &ast, true) {
                let _ = LinearDb::new(&mut diags, &ast, &ast_db);
            }
        }
    }
});
