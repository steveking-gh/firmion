// Integration tests for the std::md5 extension.
//
// Each test invokes the firmion binary and checks either the exit code or
// the exact bytes written to the output file.  The MD5 values below
// were computed with Python's hashlib.md5 and cross-checked against
// the md-5 crate used by the extension itself.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use std::fs;

    /// Resolves a path relative to the workspace root regardless of which
    /// crate's test runner sets the working directory.
    fn workspace_path(rel: &str) -> std::path::PathBuf {
        // CARGO_MANIFEST_DIR is set to the crate root (std/md5).
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

    /// Section-name form: wr std::md5(data)
    ///
    /// The section-name form hashes the full section as it exists in the
    /// image at extension-execution time, including the 16 zeroed digest
    /// placeholder bytes.  Input = [0x01, 0x02, 0x03, 0x04, 0x00 * 16].
    #[test]
    fn md5_section_form() {
        let out = run_and_read(
            "extensions/tests/md5/md5_section.firm",
            "md5_section.bin",
        );
        assert_eq!(
            out,
            vec![
                0x01, 0x02, 0x03, 0x04,                             // data bytes
                0xBD, 0x5A, 0x23, 0x7A, 0xFE, 0xB1, 0x85, 0x55,   // digest
                0xFE, 0x26, 0x2E, 0x5D, 0x03, 0x7B, 0xB6, 0x6F,
            ],
            "MD5 section-form output mismatch"
        );
    }

    /// MD5 of a single byte [0xAB]. Data is in a separate `payload` section.
    /// Input size (1 byte) is less than the 16-byte MD5 digest size.
    /// Output: 1 data byte followed by 16 digest bytes.
    #[test]
    fn md5_1_byte() {
        let out = run_and_read(
            "extensions/tests/md5/md5_1_byte.firm",
            "md5_1_byte.bin",
        );
        assert_eq!(
            out,
            vec![
                0xAB,                                               // data byte
                0x24, 0x08, 0xAD, 0x11, 0xF9, 0xEB, 0x83, 0x0D, // digest
                0xA7, 0x49, 0xE2, 0xA3, 0x6A, 0x29, 0xEF, 0xF7,
            ],
            "MD5 1-byte output mismatch"
        );
    }

    /// MD5 of 16 bytes [0x00..0x0F]. Data is in a separate `payload` section.
    /// Input size (16 bytes) equals the MD5 digest size.
    /// Output: 16 data bytes followed by 16 digest bytes.
    #[test]
    fn md5_16_byte() {
        let out = run_and_read(
            "extensions/tests/md5/md5_16_byte.firm",
            "md5_16_byte.bin",
        );
        assert_eq!(
            out,
            vec![
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, // data bytes
                0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
                0x1A, 0xC1, 0xEF, 0x01, 0xE9, 0x6C, 0xAF, 0x1B, // digest
                0xE0, 0xD3, 0x29, 0x33, 0x1A, 0x4F, 0xC2, 0xA8,
            ],
            "MD5 16-byte output mismatch"
        );
    }

    /// MD5 of 64 bytes [0x00..0x3F]. Data is in a separate `payload` section.
    /// Input size (64 bytes) is greater than the 16-byte MD5 digest size and
    /// spans multiple MD5 compression blocks, exercising multi-block hashing.
    /// Output: 64 data bytes followed by 16 digest bytes.
    #[test]
    fn md5_64_byte() {
        let out = run_and_read(
            "extensions/tests/md5/md5_64_byte.firm",
            "md5_64_byte.bin",
        );
        let mut expected: Vec<u8> = (0x00u8..=0x3F).collect();
        expected.extend_from_slice(&[
            0xB2, 0xD3, 0xF5, 0x6B, 0xC1, 0x97, 0xFD, 0x98, // digest
            0x5D, 0x59, 0x65, 0x07, 0x9B, 0x5E, 0x71, 0x48,
        ]);
        assert_eq!(out, expected, "MD5 64-byte output mismatch");
    }

    /// sizeof(std::md5) must be 16.
    #[test]
    fn md5_sizeof() {
        let src_path = workspace_path("extensions/tests/md5/md5_sizeof.firm");
        let out_path = workspace_path("md5_sizeof.bin");
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
    fn md5_ambiguous_section() {
        let src_path = workspace_path("extensions/tests/md5/md5_ambiguous_section.firm");
        Command::cargo_bin("firmion")
            .unwrap()
            .arg(&src_path)
            .assert()
            .failure()
            .stderr(predicates::str::contains("ERR_173"));
    }
}
