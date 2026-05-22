// Integration tests for the std::crc32c extension.
//
// Each test invokes the firmion binary and checks either the exit code or
// the exact bytes written to the output file.  The CRC32C values below
// were computed with the Castagnoli polynomial (0x1EDC6F41) and verified
// against the crc32c crate used by the extension itself.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use std::fs;

    /// Resolves a path relative to the workspace root regardless of which
    /// crate's test runner sets the working directory.
    fn workspace_path(rel: &str) -> std::path::PathBuf {
        // CARGO_MANIFEST_DIR is set to the crate root (std/crc32c).
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

    /// Section-name form: wr std::crc32c(data)
    ///
    /// The section-name form hashes the full section as it exists in the
    /// image at extension-execution time, including the 4 zeroed CRC
    /// placeholder bytes.  Input = [0x01,0x02,0x03,0x04,0x00,0x00,0x00,0x00].
    /// CRC32C = 0xC1EBE357, LE: [0x57, 0xE3, 0xEB, 0xC1].
    #[test]
    fn crc32c_section_form() {
        let out = run_and_read(
            "extensions/tests/crc32c/crc32c_section.firm",
            "crc32c_section.bin",
        );
        assert_eq!(
            out,
            vec![0x01, 0x02, 0x03, 0x04, 0x57, 0xE3, 0xEB, 0xC1],
            "CRC32C section-form output mismatch"
        );
    }

    /// CRC32C over [0xAA, 0xBB, 0xCC, 0xDD] = 0xF7CEEA9E (LE: 9E EA CE F7).
    /// Data is in a separate `payload` section so the extension hashes only
    /// the data bytes, not the CRC placeholder.
    #[test]
    fn crc32c_explicit_range() {
        let out = run_and_read(
            "extensions/tests/crc32c/crc32c_explicit_range.firm",
            "crc32c_explicit_range.bin",
        );
        assert_eq!(
            out,
            vec![0xAA, 0xBB, 0xCC, 0xDD, 0x9E, 0xEA, 0xCE, 0xF7],
            "CRC32C explicit-range output mismatch"
        );
    }

    /// sizeof(std::crc32c) must be 4.
    #[test]
    fn crc32c_sizeof() {
        let src_path = workspace_path("extensions/tests/crc32c/crc32c_sizeof.firm");
        let out_path = workspace_path("crc32c_sizeof.bin");
        Command::cargo_bin("firmion")
            .unwrap()
            .arg(&src_path)
            .arg("-o")
            .arg(&out_path)
            .assert()
            .success();
        fs::remove_file(&out_path).ok();
    }

    /// Wrapping each occurrence of a repeated section in a unique section
    /// resolves the ERR_173 ambiguity.  CRC32C([0x01,0x02,0x03,0x04]) =
    /// 0x29308CF4, LE: [0xF4, 0x8C, 0x30, 0x29].  Total output: 12 bytes.
    #[test]
    fn crc32c_wrapped_section() {
        let out = run_and_read(
            "extensions/tests/crc32c/crc32c_wrapped_section.firm",
            "crc32c_wrapped_section.bin",
        );
        assert_eq!(
            out,
            vec![
                0x01, 0x02, 0x03, 0x04, // data_a
                0x01, 0x02, 0x03, 0x04, // data_b
                0xF4, 0x8C, 0x30, 0x29, // CRC32C of data_a
            ],
            "CRC32C wrapped-section output mismatch"
        );
    }

    /// Section-name form is ambiguous when the named section appears more
    /// than once in the output.  Expects ERR_173 and a non-zero exit code.
    #[test]
    fn crc32c_ambiguous_section() {
        let src_path = workspace_path("extensions/tests/crc32c/crc32c_ambiguous_section.firm");
        Command::cargo_bin("firmion")
            .unwrap()
            .arg(&src_path)
            .assert()
            .failure()
            .stderr(predicates::str::contains("ERR_173"));
    }
}
