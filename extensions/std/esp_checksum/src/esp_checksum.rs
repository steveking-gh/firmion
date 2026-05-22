// std::esp_checksum -- ESP checksum extension for Firmion.
//
// Computes an XOR checksum across the payload of all ESP style segments in the
// specified section.  The first byte of the section must be the start of the
// ESP file header, i.e. magic byte 0xE9. Set offset to the byte offset of the
// first segment header.  The offset is typically either 8 for ESP8266 or 24
// for the newer ESP32 family.
//
// This extension verifies that the segment count in the ESP file header matches
// the actual number of segments in the section.
//
// ESP image header (starting at byte 0 of the section): byte 0:   magic (0xE9)
//   byte 1:   segment_count bytes 2+: remaining header fields (not read by this
//   extension)
//
// ESP segment headers are 8 bytes in length with the following format:
//  0 - 3: payload load address (little-endian) (ignored by this extension)
//  4 - 7: payload size (little-endian)
//
// Call-site syntax:  wr std::esp_checksum(<sec_name>, <offset>);
//
// Output: 1 byte, XOR (seed=0xEF) of all segment payloads in the specified
// section.
//

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use firmion_extension::{FirmionExtension, ParamArg, ParamDesc, ParamKind};
use firmion_extension::extension_registry::ExtensionRegistry;

pub struct EspChecksum;

impl FirmionExtension for EspChecksum {
    fn name(&self) -> &str {
        "std::esp_checksum"
    }

    fn size(&self) -> usize {
        1
    }

    fn params(&self) -> &[ParamDesc] {
        &[
            ParamDesc {
                name: "data",
                kind: ParamKind::Slice,
            },
            ParamDesc {
                name: "offset",
                kind: ParamKind::Int,
            },
        ]
    }

    fn execute<'a>(&self, args: &[ParamArg<'a>], out_buffer: &mut [u8]) -> Result<(), String> {
        let ParamArg::Slice { data: img } = args
            .first()
            .ok_or_else(|| "std::esp_checksum: missing data argument".to_string())?
        else {
            return Err("std::esp_checksum: args[0] must be a section slice".to_string());
        };

        let offset = match args.get(1) {
            Some(ParamArg::Int(v)) => *v as usize,
            _ => {
                return Err(
                    "std::esp_checksum: missing or wrong type for offset argument".to_string(),
                );
            }
        };

        if img.len() < 2 {
            return Err("std::esp_checksum: image too small to contain header".to_string());
        }
        if img[0] != 0xE9 {
            return Err(format!(
                "std::esp_checksum: invalid magic byte {:#04X}, expected 0xE9",
                img[0]
            ));
        }
        let seg_count = img[1] as usize;

        let mut checksum = 0xEFu8;
        let mut pos = offset;

        for seg in 0..seg_count {
            if pos + 8 > img.len() {
                return Err(format!(
                    "std::esp_checksum: segment {seg} header at offset {pos} extends past end of image"
                ));
            }
            let payload_len =
                u32::from_le_bytes([img[pos + 4], img[pos + 5], img[pos + 6], img[pos + 7]])
                    as usize;
            pos += 8;
            if pos + payload_len > img.len() {
                return Err(format!(
                    "std::esp_checksum: segment {seg} payload at offset {pos} extends past end of image"
                ));
            }
            for &b in &img[pos..pos + payload_len] {
                checksum ^= b;
            }
            pos += payload_len;
        }

        out_buffer[0] = checksum;
        Ok(())
    }
}

/// Registers `std::esp_checksum` into the given registry.
/// Call once during process startup, before compiling any scripts.
pub fn register(registry: &mut ExtensionRegistry) {
    registry.register(Box::new(EspChecksum));
}
