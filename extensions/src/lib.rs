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

#[cfg(feature = "std-crc32c")]
pub mod crc32c;
#[cfg(feature = "std-sha256")]
pub mod sha256;
#[cfg(feature = "std-md5")]
pub mod md5;
#[cfg(feature = "std-xor")]
pub mod xor;
#[cfg(feature = "std-esp-checksum")]
pub mod esp_checksum;

/// Registers all compiled-in extensions into `registry`.
/// Call once before compiling any Firmion scripts.
pub fn register_all(_registry: &mut ExtensionRegistry) {
    #[cfg(feature = "std-crc32c")]
    crc32c::register(_registry);
    #[cfg(feature = "std-sha256")]
    sha256::register(_registry);
    #[cfg(feature = "std-md5")]
    md5::register(_registry);
    #[cfg(feature = "std-xor")]
    xor::register(_registry);
    #[cfg(feature = "std-esp-checksum")]
    esp_checksum::register(_registry);
}
