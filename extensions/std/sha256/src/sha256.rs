// std::sha256 -- SHA-256 extension for Firmion.
//
// Computes a SHA-256 digest over a caller-specified image region and
// writes the 32-byte result into the output image.
//
// Call-site syntax (section-name form):
//     wr std::sha256(my_section);
//
// Output: 32 bytes, big-endian digest (standard SHA-256 byte order).

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use firmion_extension::{FirmionExtension, ParamArg, ParamDesc, ParamKind};
use firmion_extension::extension_registry::ExtensionRegistry;
use sha2::{Digest, Sha256};

pub struct Sha256Ext;

impl FirmionExtension for Sha256Ext {
    fn name(&self) -> &str {
        "std::sha256"
    }

    fn size(&self) -> usize {
        32
    }

    fn params(&self) -> &[ParamDesc] {
        &[ParamDesc {
            name: "data",
            kind: ParamKind::Slice,
        }]
    }

    fn execute<'a>(&self, args: &[ParamArg<'a>], out_buffer: &mut [u8]) -> Result<(), String> {
        let ParamArg::Slice { data: img_buffer } = args
            .first()
            .ok_or("std::sha256: expected ParamArg::Slice as args[0]".to_string())?
        else {
            return Err(
                "std::sha256: args[0] must be ParamArg::Slice (use section-name form)".to_string(),
            );
        };
        let digest = Sha256::digest(img_buffer);
        out_buffer.copy_from_slice(&digest);
        Ok(())
    }
}

/// Registers `std::sha256` into the given registry.
/// Call this once during process startup, before compiling any scripts.
pub fn register(registry: &mut ExtensionRegistry) {
    registry.register(Box::new(Sha256Ext));
}
