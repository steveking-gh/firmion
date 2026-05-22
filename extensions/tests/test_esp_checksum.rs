// Integration tests for the std::esp_checksum extension.
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

    /// 3-segment ESP image: seg0=7 bytes, seg1=10 bytes, seg2=7 bytes.
    /// seed=0xEF; fold: seg0->0xEA, seg1->0xFB, seg2->0x32.
    /// Total image = 24 (file_hdr) + 15 (seg0) + 18 (seg1) + 15 (seg2) + 1 (chk) = 73 bytes.
    #[test]
    fn esp_checksum_section() {
        let out = run_and_read(
            "extensions/tests/esp_checksum/esp_checksum_section.firm",
            "esp_checksum_section.bin",
        );
        assert_eq!(out.len(), 73, "unexpected image size");
        assert_eq!(out[72], 0x32, "checksum byte mismatch");
    }

    /// sizeof(std::esp_checksum) must be 1.
    #[test]
    fn esp_checksum_sizeof() {
        let src_path = workspace_path("extensions/tests/esp_checksum/esp_checksum_sizeof.firm");
        let out_path = workspace_path("esp_checksum_sizeof.bin");
        Command::cargo_bin("firmion")
            .unwrap()
            .arg(&src_path)
            .arg("-o")
            .arg(&out_path)
            .assert()
            .success();
        fs::remove_file(&out_path).ok();
    }

}
