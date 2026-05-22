// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use std::collections::HashMap;

pub use crate::{FirmionExtension, ParamArg, ParamDesc, ParamKind};

/// Owns a registered extension alongside its cached metadata.
///
/// Firmion calls `size()` and `params()` exactly once -- at registration time --
/// and stores the results here.  All internal lookups use cached values rather
/// than calling the extension again.
pub struct ExtensionEntry {
    pub extension: Box<dyn FirmionExtension>,
    pub cached_size: usize,
    /// Parameter declarations cached from `FirmionExtension::params()`.
    /// Empty when the extension opts out of named-argument enforcement.
    pub cached_params: Vec<ParamDesc>,
}

impl ExtensionEntry {
    /// Returns the extension's fully-qualified name.
    pub fn name(&self) -> &str {
        self.extension.name()
    }
}

/// A registry that owns and provides lookup for all available Firmion extensions.
pub struct ExtensionRegistry {
    extensions: HashMap<String, ExtensionEntry>,
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionRegistry {
    /// Creates a new, empty extension registry.
    pub fn new() -> Self {
        Self {
            extensions: HashMap::new(),
        }
    }

    /// Registers an extension. Calls `FirmionExtension::size` exactly once and
    /// caches the result. Panics if an extension with the same name is already
    /// registered.
    pub fn register(&mut self, ext: Box<dyn FirmionExtension>) {
        let name = ext.name().to_string();
        assert!(
            !self.extensions.contains_key(&name),
            "Extension '{}' is already registered",
            name
        );
        let cached_size = ext.size();
        let cached_params = ext.params().to_vec();
        self.extensions.insert(
            name,
            ExtensionEntry {
                extension: ext,
                cached_size,
                cached_params,
            },
        );
    }

    /// Retrieves a registered extension entry by its fully-qualified name.
    pub fn get(&self, name: &str) -> Option<&ExtensionEntry> {
        self.extensions.get(name)
    }

    /// Returns all registered extension names in sorted order.
    pub fn sorted_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.extensions.keys().map(String::as_str).collect();
        names.sort();
        names
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParamArg;
    use crate::test_mocks::*;

    #[test]
    fn test_registry_registration_and_lookup() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockCrc::new()));

        assert!(
            reg.get("firmion::test_crc").is_some(),
            "Should find registered extension"
        );
        assert!(
            reg.get("missing::func").is_none(),
            "Should reject unregistered extensions"
        );
    }

    #[test]
    #[should_panic(expected = "Extension 'firmion::test_crc' is already registered")]
    fn test_duplicate_registration_panics() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockCrc::new()));
        reg.register(Box::new(MockCrc::new()));
    }

    /// size() must be called exactly once -- at registration -- and never again.
    #[test]
    fn test_size_called_once_on_register() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockCrc::new()));
        // Retrieve the entry multiple times; the Cell counter must stay at 1.
        let _ = reg.get("firmion::test_crc");
        let _ = reg.get("firmion::test_crc");
        // MockCrc::size() would panic on a second call, so reaching here is the assertion.
    }

    /// cached_size must equal the value size() returned.
    #[test]
    fn test_cached_size_matches_extension() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockCrc::new()));
        let entry = reg.get("firmion::test_crc").unwrap();
        assert_eq!(entry.cached_size, 4);
    }

    #[test]
    fn test_valid_extension_execution() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockCrc::new()));
        let entry = reg.get("firmion::test_crc").unwrap();

        let args = vec![ParamArg::Int(0xDEADBEEF_u64)];
        let mut out = vec![0u8; 4];
        entry.extension.execute(&args, &mut out).unwrap();
        assert_eq!(out, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_invalid_argument_count() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockCrc::new()));
        let entry = reg.get("firmion::test_crc").unwrap();

        // MockCrc expects exactly 1 arg; provide 2.
        let args = vec![ParamArg::Int(1), ParamArg::Int(2)];
        let mut out = vec![0u8; 4];
        let res = entry.extension.execute(&args, &mut out);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Expected exactly 1 argument for CRC");
    }

    #[test]
    fn test_invalid_output_buffer() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockCrc::new()));
        let entry = reg.get("firmion::test_crc").unwrap();

        let args = vec![ParamArg::Int(1)];
        let mut out = vec![0u8; 2]; // MockCrc expects 4 bytes
        let res = entry.extension.execute(&args, &mut out);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Expected 4 bytes of output space");
    }

    #[test]
    fn test_ranged_sum_execution() {
        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockRangedSum::new()));
        let entry = reg.get("firmion::test_ranged_sum").unwrap();
        assert_eq!(entry.cached_size, 8);

        let img = vec![0x01u8, 0x02, 0x03, 0x04];
        let args = vec![ParamArg::Slice { data: &img }];
        let mut out = vec![0u8; 8];
        entry.extension.execute(&args, &mut out).unwrap();
        let sum = u64::from_le_bytes(out.try_into().unwrap());
        assert_eq!(sum, 10, "Sum of 1+2+3+4 must be 10");
    }

    #[test]
    fn test_logging_and_error_reporting() {
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::fmt::MakeWriter;

        #[derive(Clone)]
        struct MockWriter {
            logs: Arc<Mutex<Vec<u8>>>,
        }

        impl std::io::Write for MockWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.logs.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl<'a> MakeWriter<'a> for MockWriter {
            type Writer = Self;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let logs = Arc::new(Mutex::new(Vec::new()));
        let writer = MockWriter { logs: logs.clone() };

        // try_init may fail if another test already initialized the subscriber.
        let _ = tracing_subscriber::fmt()
            .with_writer(writer)
            .with_max_level(tracing::Level::INFO)
            .try_init();

        let mut reg = ExtensionRegistry::new();
        reg.register(Box::new(MockLogger::new()));

        let entry = reg.get("firmion::test_logger").unwrap();
        let args: Vec<ParamArg<'_>> = vec![];
        let res = entry.extension.execute(&args, &mut []);

        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Intentional mock fallback error");

        let log_output = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
        assert!(log_output.contains("MockLogger executed successfully via tracing API"));
    }
}
