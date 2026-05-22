// Integration tests for the std::sha256 extension.
//
// Each test invokes the firmion binary and checks either the exit code or
// the exact bytes written to the output file.  The SHA-256 values below
// were computed with Python's hashlib.sha256 and cross-checked against
// the sha2 crate used by the extension itself.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use std::fs;

    /// Resolves a path relative to the workspace root regardless of which
    /// crate's test runner sets the working directory.
    fn workspace_path(rel: &str) -> std::path::PathBuf {
        // CARGO_MANIFEST_DIR is set to the crate root (std/sha256).
        // Two levels up is the workspace root.
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest.join("..").join(rel)
    }

    /// Runs the firmion binary on a .firm file and asserts success.
    /// Returns the bytes written to the output file, then removes it.
    fn run_and_read(src: &str, out: &str) -> Vec<u8> {
        let src_path = workspace_path(src);
        let out_path = workspace_path(out);
        Command::cargo_bin("firmion")
            .unwrap()
            .arg(&src_path)
            .arg("-o")
            .arg(&out_path)
            .assert()
            .success();
        let bytes = fs::read(&out_path).expect("output file not found");
        fs::remove_file(&out_path).unwrap();
        bytes
    }

    /// Section-name form: wr std::sha256(data)
    ///
    /// The section-name form hashes the full section as it exists in the
    /// image at extension-execution time, including the 32 zeroed digest
    /// placeholder bytes.  Input = [0x01, 0x02, 0x03, 0x04, 0x00 * 32].
    #[test]
    fn sha256_section_form() {
        let out = run_and_read(
            "extensions/tests/sha256/sha256_section.firm",
            "sha256_section.bin",
        );
        assert_eq!(
            out,
            vec![
                0x01, 0x02, 0x03, 0x04, // data bytes
                0x0C, 0xD7, 0xDB, 0x9E, 0xEE, 0x47, 0x56, 0x54, // digest
                0xB6, 0x55, 0xAB, 0xF1, 0x48, 0xDD, 0xBE, 0x05,
                0x63, 0xDC, 0xBE, 0xC0, 0xAB, 0x7E, 0xA3, 0xEC,
                0xD7, 0x25, 0x4B, 0x17, 0x51, 0x69, 0x95, 0x2D,
            ],
            "SHA-256 section-form output mismatch"
        );
    }

    /// SHA-256 of a single byte [0xAB]. Data is in a separate `payload` section
    /// so the extension hashes only that byte, not the digest placeholder.
    /// Output: 1 data byte followed by 32 digest bytes.
    #[test]
    fn sha256_1_byte() {
        let out = run_and_read(
            "extensions/tests/sha256/sha256_1_byte.firm",
            "sha256_1_byte.bin",
        );
        assert_eq!(
            out,
            vec![
                0xAB,                                               // data byte
                0x08, 0x7D, 0x80, 0xF7, 0xF1, 0x82, 0xDD, 0x44, // digest
                0xF1, 0x84, 0xAA, 0x86, 0xCA, 0x34, 0x48, 0x88,
                0x53, 0xEB, 0xCC, 0x04, 0xF0, 0xC6, 0x0D, 0x52,
                0x94, 0x91, 0x9A, 0x46, 0x6B, 0x46, 0x38, 0x31,
            ],
            "SHA-256 1-byte output mismatch"
        );
    }

    /// SHA-256 of 16 bytes [0x00..0x0F]. Data is in a separate `payload` section.
    /// Output: 16 data bytes followed by 32 digest bytes.
    #[test]
    fn sha256_16_byte() {
        let out = run_and_read(
            "extensions/tests/sha256/sha256_16_byte.firm",
            "sha256_16_byte.bin",
        );
        assert_eq!(
            out,
            vec![
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, // data bytes
                0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
                0xBE, 0x45, 0xCB, 0x26, 0x05, 0xBF, 0x36, 0xBE, // digest
                0xBD, 0xE6, 0x84, 0x84, 0x1A, 0x28, 0xF0, 0xFD,
                0x43, 0xC6, 0x98, 0x50, 0xA3, 0xDC, 0xE5, 0xFE,
                0xDB, 0xA6, 0x99, 0x28, 0xEE, 0x3A, 0x89, 0x91,
            ],
            "SHA-256 16-byte output mismatch"
        );
    }

    /// SHA-256 of 64 bytes [0x00..0x3F]. Data is in a separate `payload` section.
    /// 64 bytes spans two SHA-256 compression blocks, exercising multi-block hashing.
    /// Output: 64 data bytes followed by 32 digest bytes.
    #[test]
    fn sha256_64_byte() {
        let out = run_and_read(
            "extensions/tests/sha256/sha256_64_byte.firm",
            "sha256_64_byte.bin",
        );
        let mut expected: Vec<u8> = (0x00u8..=0x3F).collect();
        expected.extend_from_slice(&[
            0xFD, 0xEA, 0xB9, 0xAC, 0xF3, 0x71, 0x03, 0x62, // digest
            0xBD, 0x26, 0x58, 0xCD, 0xC9, 0xA2, 0x9E, 0x8F,
            0x9C, 0x75, 0x7F, 0xCF, 0x98, 0x11, 0x60, 0x3A,
            0x8C, 0x44, 0x7C, 0xD1, 0xD9, 0x15, 0x11, 0x08,
        ]);
        assert_eq!(out, expected, "SHA-256 64-byte output mismatch");
    }

    /// sizeof(std::sha256) must be 32.
    #[test]
    fn sha256_sizeof() {
        let src_path = workspace_path("extensions/tests/sha256/sha256_sizeof.firm");
        let out_path = workspace_path("sha256_sizeof.bin");
        Command::cargo_bin("firmion")
            .unwrap()
            .arg(&src_path)
            .arg("-o")
            .arg(&out_path)
            .assert()
            .success();
        fs::remove_file(&out_path).ok();
    }

    /// Section-name form is ambiguous when the named section appears more
    /// than once in the output.  Expects ERR_173 and a non-zero exit code.
    #[test]
    fn sha256_ambiguous_section() {
        let src_path = workspace_path("extensions/tests/sha256/sha256_ambiguous_section.firm");
        Command::cargo_bin("firmion")
            .unwrap()
            .arg(&src_path)
            .assert()
            .failure()
            .stderr(predicates::str::contains("ERR_173"));
    }
}
