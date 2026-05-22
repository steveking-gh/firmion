// std::md5 -- MD5 extension for Firmion.
//
// Computes an MD5 digest over a caller-specified image region and
// writes the 16-byte result into the output image.
//
// Call-site syntax (section-name form):
//     wr std::md5(my_section);
//
// Output: 16 bytes, standard MD5 byte order.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use firmion_extension::{FirmionExtension, ParamArg, ParamDesc, ParamKind};
use firmion_extension::extension_registry::ExtensionRegistry;
use md5::{Digest, Md5};

pub struct Md5Ext;

impl FirmionExtension for Md5Ext {
    fn name(&self) -> &str {
        "std::md5"
    }

    fn size(&self) -> usize {
        16
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
            .ok_or("std::md5: expected ParamArg::Slice as args[0]".to_string())?
        else {
            return Err(
                "std::md5: args[0] must be ParamArg::Slice (use section-name form)".to_string(),
            );
        };
        let digest = Md5::digest(img_buffer);
        out_buffer.copy_from_slice(&digest);
        Ok(())
    }
}

/// Registers `std::md5` into the given registry.
/// Call once during process startup, before compiling any scripts.
pub fn register(registry: &mut ExtensionRegistry) {
    registry.register(Box::new(Md5Ext));
}
