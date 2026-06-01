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
            let Some(header_end) = pos.checked_add(8) else {
                return Err("std::esp_checksum: offset overflow".to_string());
            };
            if header_end > img.len() {
                return Err(format!(
                    "std::esp_checksum: segment {seg} header at offset {pos} extends past end of image"
                ));
            }
            let payload_len =
                u32::from_le_bytes([img[pos + 4], img[pos + 5], img[pos + 6], img[pos + 7]])
                    as usize;
            pos = header_end;
            let Some(payload_end) = pos.checked_add(payload_len) else {
                return Err("std::esp_checksum: payload length overflow".to_string());
            };
            if payload_end > img.len() {
                return Err(format!(
                    "std::esp_checksum: segment {seg} payload at offset {pos} extends past end of image"
                ));
            }
            for &b in &img[pos..payload_end] {
                checksum ^= b;
            }
            pos = payload_end;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_esp_checksum_valid() {
        let extension = EspChecksum;
        let mut out_buffer = [0u8; 1];
        // 1 segment, magic = 0xE9, seg_count = 1
        // segment header: load addr = [0, 0, 0, 0], size = [2, 0, 0, 0] (2 bytes)
        // payload: [0xAA, 0xBB]
        // offset = 8
        let img = vec![
            0xE9, 1, 0, 0, 0, 0, 0, 0, // file header
            0, 0, 0, 0, 2, 0, 0, 0,    // segment header: size = 2
            0xAA, 0xBB,                // payload
        ];
        let args = vec![
            ParamArg::Slice { data: &img },
            ParamArg::Int(8),
        ];
        let res = extension.execute(&args, &mut out_buffer);
        assert!(res.is_ok());
        // checksum = 0xEF ^ 0xAA ^ 0xBB = 0xFE
        assert_eq!(out_buffer[0], 0xFE);
    }

    #[test]
    fn test_esp_checksum_offset_overflow() {
        let extension = EspChecksum;
        let mut out_buffer = [0u8; 1];
        let img = vec![0xE9, 1, 0, 0];
        let args = vec![
            ParamArg::Slice { data: &img },
            ParamArg::Int(usize::MAX as u64),
        ];
        let res = extension.execute(&args, &mut out_buffer);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("offset overflow"));
    }

    #[test]
    fn test_esp_checksum_header_extends_past() {
        let extension = EspChecksum;
        let mut out_buffer = [0u8; 1];
        let img = vec![0xE9, 1, 0, 0];
        let args = vec![
            ParamArg::Slice { data: &img },
            ParamArg::Int(2),
        ];
        let res = extension.execute(&args, &mut out_buffer);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("extends past end of image"));
    }

    #[test]
    fn test_esp_checksum_payload_extends_past() {
        let extension = EspChecksum;
        let mut out_buffer = [0u8; 1];
        // 1 segment, size = 100 bytes (extends past end of image)
        let img = vec![
            0xE9, 1, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 100, 0, 0, 0,
        ];
        let args = vec![
            ParamArg::Slice { data: &img },
            ParamArg::Int(8),
        ];
        let res = extension.execute(&args, &mut out_buffer);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("extends past end of image"));
    }
}
