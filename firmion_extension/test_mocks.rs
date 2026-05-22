// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use super::*;
use crate::{ParamArg, ParamDesc, ParamKind};
use crate::extension_registry::ExtensionRegistry;
use std::cell::Cell;

pub struct MockCrc {
    size_call_count: Cell<usize>,
}

impl MockCrc {
    pub fn new() -> Self {
        Self {
            size_call_count: Cell::new(0),
        }
    }
}

impl Default for MockCrc {
    fn default() -> Self {
        Self::new()
    }
}

impl FirmionExtension for MockCrc {
    fn name(&self) -> &str {
        "firmion::test_crc"
    }

    fn size(&self) -> usize {
        let prev = self.size_call_count.get();
        assert_eq!(prev, 0, "MockCrc::size() called more than once");
        self.size_call_count.set(prev + 1);
        4
    }

    fn params(&self) -> &[ParamDesc] {
        &[ParamDesc { name: "value", kind: ParamKind::Int }]
    }

    fn execute<'a>(&self, args: &[ParamArg<'a>], out: &mut [u8]) -> Result<(), String> {
        if args.len() != 1 {
            return Err("Expected exactly 1 argument for CRC".to_string());
        }
        if out.len() != 4 {
            return Err("Expected 4 bytes of output space".to_string());
        }
        let ParamArg::Int(v) = args[0] else {
            return Err("Expected Int argument for CRC".to_string());
        };
        let val = v as u32;
        out.copy_from_slice(&val.to_be_bytes());
        Ok(())
    }
}

pub struct MockLogger {
    size_call_count: Cell<usize>,
}

impl MockLogger {
    pub fn new() -> Self {
        Self {
            size_call_count: Cell::new(0),
        }
    }
}

impl Default for MockLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl FirmionExtension for MockLogger {
    fn name(&self) -> &str {
        "firmion::test_logger"
    }

    fn size(&self) -> usize {
        let prev = self.size_call_count.get();
        assert_eq!(prev, 0, "MockLogger::size() called more than once");
        self.size_call_count.set(prev + 1);
        0
    }

    fn execute<'a>(&self, _args: &[ParamArg<'a>], _out: &mut [u8]) -> Result<(), String> {
        tracing::info!("MockLogger executed successfully via tracing API");
        Err("Intentional mock fallback error".to_string())
    }
}

/// Reads the caller-specified image slice and writes each byte + 1 to the
/// output buffer. args[0] must be ParamArg::Slice; the output buffer receives
/// the first cached_size bytes of that section, each incremented by 1.
pub struct MockIncrement {
    size_call_count: Cell<usize>,
}

impl MockIncrement {
    pub fn new() -> Self {
        Self {
            size_call_count: Cell::new(0),
        }
    }
}

impl Default for MockIncrement {
    fn default() -> Self {
        Self::new()
    }
}

impl FirmionExtension for MockIncrement {
    fn name(&self) -> &str {
        "firmion::test_increment"
    }

    fn size(&self) -> usize {
        let prev = self.size_call_count.get();
        assert_eq!(prev, 0, "MockIncrement::size() called more than once");
        self.size_call_count.set(prev + 1);
        16
    }

    fn params(&self) -> &[ParamDesc] {
        &[ParamDesc { name: "data", kind: ParamKind::Slice }]
    }

    fn execute<'a>(&self, args: &[ParamArg<'a>], out_buffer: &mut [u8]) -> Result<(), String> {
        let ParamArg::Slice { data: img_buffer } = args.first().ok_or(
            "MockIncrement: expected ParamArg::Slice as args[0]".to_string(),
        )? else {
            return Err("MockIncrement: args[0] must be ParamArg::Slice".to_string());
        };
        assert!(
            img_buffer.len() >= out_buffer.len(),
            "MockIncrement: img_buffer must be at least as large as out_buffer (got {} vs {})",
            img_buffer.len(),
            out_buffer.len()
        );
        for (out, src) in out_buffer.iter_mut().zip(img_buffer.iter()) {
            *out = src.wrapping_add(1);
        }
        Ok(())
    }
}

/// Sums every byte in the caller-specified image slice and writes the result
/// as a little-endian u64. args[0] must be ParamArg::Slice.
pub struct MockRangedSum {
    size_call_count: Cell<usize>,
}

impl MockRangedSum {
    pub fn new() -> Self {
        Self {
            size_call_count: Cell::new(0),
        }
    }
}

impl Default for MockRangedSum {
    fn default() -> Self {
        Self::new()
    }
}

impl FirmionExtension for MockRangedSum {
    fn name(&self) -> &str {
        "firmion::test_ranged_sum"
    }

    fn size(&self) -> usize {
        let prev = self.size_call_count.get();
        assert_eq!(prev, 0, "MockRangedSum::size() called more than once");
        self.size_call_count.set(prev + 1);
        8
    }

    fn params(&self) -> &[ParamDesc] {
        &[ParamDesc { name: "data", kind: ParamKind::Slice }]
    }

    fn execute<'a>(&self, args: &[ParamArg<'a>], out_buffer: &mut [u8]) -> Result<(), String> {
        let ParamArg::Slice { data: img_buffer } = args.first().ok_or(
            "MockRangedSum: expected ParamArg::Slice as args[0]".to_string(),
        )? else {
            return Err("MockRangedSum: args[0] must be ParamArg::Slice".to_string());
        };
        assert_eq!(
            out_buffer.len(),
            8,
            "MockRangedSum: out_buffer must be exactly 8 bytes"
        );
        let sum: u64 = img_buffer.iter().map(|&b| b as u64).sum();
        out_buffer.copy_from_slice(&sum.to_le_bytes());
        Ok(())
    }
}

/// Rejects a zero-length input slice with an error. args[0] must be
/// ParamArg::Slice. Used to verify that extensions can reject empty sections.
pub struct MockRejectEmpty {
    size_call_count: Cell<usize>,
}

impl MockRejectEmpty {
    pub fn new() -> Self {
        Self {
            size_call_count: Cell::new(0),
        }
    }
}

impl Default for MockRejectEmpty {
    fn default() -> Self {
        Self::new()
    }
}

impl FirmionExtension for MockRejectEmpty {
    fn name(&self) -> &str {
        "firmion::test_reject_empty"
    }

    fn size(&self) -> usize {
        let prev = self.size_call_count.get();
        assert_eq!(prev, 0, "MockRejectEmpty::size() called more than once");
        self.size_call_count.set(prev + 1);
        4
    }

    fn params(&self) -> &[ParamDesc] {
        &[ParamDesc { name: "data", kind: ParamKind::Slice }]
    }

    fn execute<'a>(&self, args: &[ParamArg<'a>], out_buffer: &mut [u8]) -> Result<(), String> {
        let ParamArg::Slice { data: img_buffer } = args.first().ok_or(
            "MockRejectEmpty: expected ParamArg::Slice as args[0]".to_string(),
        )? else {
            return Err("MockRejectEmpty: args[0] must be ParamArg::Slice".to_string());
        };
        if img_buffer.is_empty() {
            return Err("firmion::test_reject_empty: input range must not be empty".to_string());
        }
        out_buffer.fill(0xAB);
        Ok(())
    }
}

/// Sums eight u64 arguments and writes the result as a little-endian u64.
/// Rejects calls with any argument count other than 8.
pub struct MockSum8;

impl FirmionExtension for MockSum8 {
    fn name(&self) -> &str {
        "firmion::test_sum8"
    }

    fn size(&self) -> usize {
        8
    }

    fn params(&self) -> &[ParamDesc] {
        &[
            ParamDesc { name: "a", kind: ParamKind::Int },
            ParamDesc { name: "b", kind: ParamKind::Int },
            ParamDesc { name: "c", kind: ParamKind::Int },
            ParamDesc { name: "d", kind: ParamKind::Int },
            ParamDesc { name: "e", kind: ParamKind::Int },
            ParamDesc { name: "f", kind: ParamKind::Int },
            ParamDesc { name: "g", kind: ParamKind::Int },
            ParamDesc { name: "h", kind: ParamKind::Int },
        ]
    }

    fn execute<'a>(&self, args: &[ParamArg<'a>], out: &mut [u8]) -> Result<(), String> {
        if args.len() != 8 {
            return Err(format!(
                "firmion::test_sum8: expected 8 args, got {}",
                args.len()
            ));
        }
        let mut sum: u64 = 0;
        for (i, arg) in args.iter().enumerate() {
            let ParamArg::Int(v) = arg else {
                return Err(format!(
                    "firmion::test_sum8: arg {} must be ParamArg::Int",
                    i
                ));
            };
            sum = sum.wrapping_add(*v);
        }
        out.copy_from_slice(&sum.to_le_bytes());
        Ok(())
    }
}

/// Writes the constant value 0x12345678 (big-endian) with no call-site args.
pub struct MockNoArgs;

impl FirmionExtension for MockNoArgs {
    fn name(&self) -> &str {
        "firmion::no_args"
    }

    fn size(&self) -> usize {
        4
    }

    fn execute<'a>(&self, _args: &[ParamArg<'a>], out: &mut [u8]) -> Result<(), String> {
        out.copy_from_slice(&0x12345678u32.to_be_bytes());
        Ok(())
    }
}

/// Returns `usize::MAX` from `size()` to force a u64 overflow in the iterate
/// location counter when written after any prior byte. Used to test ERR_177.
pub struct MockHugeExt;

impl FirmionExtension for MockHugeExt {
    fn name(&self) -> &str {
        "firmion::test_huge_ext"
    }

    fn size(&self) -> usize {
        usize::MAX
    }

    fn execute<'a>(&self, _args: &[ParamArg<'a>], _out: &mut [u8]) -> Result<(), String> {
        unreachable!("MockHugeExt::execute should never be called: iterate fails first");
    }
}

pub fn register_test_extensions(reg: &mut ExtensionRegistry) {
    reg.register(Box::new(MockCrc::new()));
    reg.register(Box::new(MockLogger::new()));
    reg.register(Box::new(MockHugeExt));
    reg.register(Box::new(MockNoArgs));
    reg.register(Box::new(MockSum8));
    reg.register(Box::new(MockIncrement::new()));
    reg.register(Box::new(MockRangedSum::new()));
    reg.register(Box::new(MockRejectEmpty::new()));
}
