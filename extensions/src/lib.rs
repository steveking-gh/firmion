// Extension registration for Firmion.
//
// This crate is the single place where all compiled-in extensions are
// registered with Firmion's extension registry.  To add a new extension:
//
//   1. Create a new crate implementing `FirmionExtension` or
//      `FirmionRangedExtension` from the `firmion_extension` crate.
//   2. Add it as a dependency in this file's Cargo.toml.
//   3. Call its `register` function inside `register_all` below.
//
// `process.rs` calls `register_all` once at startup and does not need
// to know about individual extensions.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use firmion_extension::extension_registry::ExtensionRegistry;

/// Registers all compiled-in extensions into `registry`.
/// Call once before compiling any Firmion scripts.
pub fn register_all(registry: &mut ExtensionRegistry) {
    #[cfg(feature = "std-crc32c")]
    std_crc32c::register(registry);
    #[cfg(feature = "std-sha256")]
    std_sha256::register(registry);
    #[cfg(feature = "std-md5")]
    std_md5::register(registry);
    #[cfg(feature = "std-xor")]
    std_xor::register(registry);
    #[cfg(feature = "std-esp-checksum")]
    std_esp_checksum::register(registry);
}
