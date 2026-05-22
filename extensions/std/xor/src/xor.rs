// std::xor -- XOR checksum extension for Firmion.
//
// Computes an XOR checksum over a caller-specified image region and
// writes the 1-byte result into the output image.
//
// Call-site syntax (section-name form):
//     wr std::xor(my_section);
//
// Output: 1 byte, XOR of all bytes in the region.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use firmion_extension::{FirmionExtension, ParamArg, ParamDesc, ParamKind};
use firmion_extension::extension_registry::ExtensionRegistry;

pub struct Xor;

impl FirmionExtension for Xor {
    fn name(&self) -> &str {
        "std::xor"
    }

    fn size(&self) -> usize {
        1
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
            .ok_or("std::xor: expected ParamArg::Slice as args[0]".to_string())?
        else {
            return Err(
                "std::xor: args[0] must be ParamArg::Slice (use section-name form)".to_string(),
            );
        };
        out_buffer[0] = img_buffer.iter().fold(0u8, |acc, &b| acc ^ b);
        Ok(())
    }
}

/// Registers `std::xor` into the given registry.
/// Call once during process startup, before compiling any scripts.
pub fn register(registry: &mut ExtensionRegistry) {
    registry.register(Box::new(Xor));
}
