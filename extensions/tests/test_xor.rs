// Integration tests for the std::xor extension.
//
// Each test invokes the firmion binary and checks either the exit code or
// the exact bytes written to the output file.  The XOR values below
// were computed by hand and verified against the fold used by the extension.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use std::fs;

    /// Resolves a path relative to the workspace root regardless of which
    /// crate's test runner sets the working directory.
    fn workspace_path(rel: &str) -> std::path::PathBuf {
        // CARGO_MANIFEST_DIR is set to the crate root (std/xor).
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

    /// Section-name form: wr std::xor(data)
    ///
    /// The section-name form XORs the full section as it exists in the
    /// image at extension-execution time, including the 1 zeroed checksum
    /// placeholder byte.  Input = [0x01, 0x02, 0x03, 0x04, 0x00].
    /// XOR = 0x01 ^ 0x02 ^ 0x03 ^ 0x04 ^ 0x00 = 0x04.
    #[test]
    fn xor_section_form() {
        let out = run_and_read("extensions/tests/xor/xor_section.firm", "xor_section.bin");
        assert_eq!(
            out,
            vec![0x01, 0x02, 0x03, 0x04, 0x04],
            "XOR section-form output mismatch"
        );
    }

    /// XOR over [0x01, 0x02, 0x04, 0x08] = 0x0F.
    /// Data is in a separate `payload` section so the extension XORs only
    /// the data bytes, not the checksum placeholder.
    #[test]
    fn xor_explicit_range() {
        let out = run_and_read(
            "extensions/tests/xor/xor_explicit_range.firm",
            "xor_explicit_range.bin",
        );
        assert_eq!(
            out,
            vec![0x01, 0x02, 0x04, 0x08, 0x0F],
            "XOR explicit-range output mismatch"
        );
    }

    /// sizeof(std::xor) must be 1.
    #[test]
    fn xor_sizeof() {
        let src_path = workspace_path("extensions/tests/xor/xor_sizeof.firm");
        let out_path = workspace_path("xor_sizeof.bin");
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
    fn xor_ambiguous_section() {
        let src_path = workspace_path("extensions/tests/xor/xor_ambiguous_section.firm");
        Command::cargo_bin("firmion")
            .unwrap()
            .arg(&src_path)
            .assert()
            .failure()
            .stderr(predicates::str::contains("ERR_173"));
    }
}
