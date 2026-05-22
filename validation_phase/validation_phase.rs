// Assert validation and pre-output print phase.
//
// ValidationPhase walks the IR in source order, firing print and assert
// instructions that appear before the IRKind::Output sentinel.  Print output
// is visible even when a later assert fails.  The phase stops on the first
// failed assert so that prints and notes preceding it have already appeared.
//
// NOTE: If extension return values are implemented, keep ValidationPhase
// ordered after execute_extensions so extension output flows into ParmValDb
// before assert evaluation.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use anyhow::{Result, anyhow};
use diags::Diags;
use ir::IRKind;
use irdb::IRDb;
use const_eval::ireval::{ParmValDb, evaluate_string_expr, execute_assert};

#[allow(unused_imports)]
use tracing::trace;

pub struct ValidationPhase {}

impl ValidationPhase {
    pub fn validate(argvaldb: &ParmValDb, irdb: &IRDb, diags: &mut Diags) -> Result<()> {
        trace!("ValidationPhase::validate:");
        for ir in &irdb.ir_vec {
            match ir.kind {
                IRKind::Print if !diags.noprint => {
                    if let Some(s) =
                        evaluate_string_expr(&argvaldb.parms, &irdb.parms, &ir.operands, diags)
                    {
                        print!("{}", s);
                    }
                }
                IRKind::Trace if diags.trace_enabled() => {
                    if let Some(mut s) =
                        evaluate_string_expr(&argvaldb.parms, &irdb.parms, &ir.operands, diags)
                    {
                        let prefix = format!("[Trace-{}] ", diags.trace_iteration);
                        s.insert_str(0, &prefix);
                        print!("{}", s);
                    }
                }
                IRKind::Assert
                    if !execute_assert(&argvaldb.parms, &irdb.parms, &irdb.ir_vec, ir, diags) =>
                {
                    return Err(anyhow!("Error detected"));
                }
                IRKind::Output => break,
                _ => {}
            }
        }
        Ok(())
    }

}
