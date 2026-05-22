// Extension API for the Firmion binary image compiler.
//
// This crate defines the public API for Firmion extension authors.
//
// FirmionExtension -- the single trait all extensions implement.
//
// ExtArg -- typed argument passed to execute().  Each parameter passed
// to an extension in Firmion code maps to one ExtArg passed to the extension
// by the compiler.
//
// Argument types:
//   Int(u64)                     -- numeric expression (u64, i64, or integer const)
//   Str(&str)                    -- quoted string const
//   Slice { start, len, data }   -- Data slice from the in-flight output data.
//                                   Firmion automatically converts sections names
//                                   into slices.
//
// Args may appear in any order.  Firmion resolves the section name to its
// location and provides the image bytes without extension authors having to
// compute offsets.
//
// Firmion registers extensions at startup via the ExtensionRegistry.
// Firmion calls size() exactly once during registration and caches
// the result.  The `firmion` and `std` namespaces are reserved.
//
// Named parameters:
//   An extension declares named parameters via params(), returning a slice of
//   ParamDesc values.  Each ParamDesc pairs a name with a ParamKind.
//   ParamKind::Slice declares that a parameter accepts a sequence of bytes
//   (supplied at call sites as a section name).  ParamKind::Int and
//   ParamKind::Str declare numeric and string parameters respectively.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

pub mod extension_registry;
pub mod test_mocks;

/// The kind of a declared extension parameter.
///
/// Used in [`ParamDesc`] to specify what type of argument a parameter accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A numeric argument: u64, i64, or an untyped integer constant.
    Int,
    /// A quoted string constant.
    Str,
    /// A sequence of bytes with starting offset and length.
    Slice,
}

/// Describes one declared parameter of an extension.
///
/// An extension returns a slice of `ParamDesc` from [`FirmionExtension::params`]
/// to opt into named-argument call sites.
#[derive(Debug, Clone, Copy)]
pub struct ParamDesc {
    /// The parameter name used at call sites: `foo::bar(name=value)`.
    pub name: &'static str,
    /// The kind of value the parameter accepts.
    pub kind: ParamKind,
}

/// The argument type passed to an extension at execution time.
///
/// Each Firmion call-site argument produces exactly one `ExtArg`.
#[derive(Debug)]
pub enum ParamArg<'a> {
    /// A numeric argument: u64, i64, or an untyped integer constant.
    Int(u64),
    /// A quoted string constant.
    Str(&'a str),
    /// A slice which includes an immutable slice of the output data.
    Slice {
        /// Read-only view of the output image at `start..start+len`.
        data: &'a [u8],
    },
}

/// Implement this trait to create a Firmion extension.
///
/// An extension writes a fixed number of bytes into the output image.
/// Arguments arrive as typed [`ExtArg`] values corresponding 1:1 to the
/// Firmion call-site arguments.  A section name argument delivers the image
/// bytes at that section directly in the [`ExtArg::Section`] variant.
///
/// # Example: numeric argument
///
/// ```rust
/// use firmion_extension::{FirmionExtension, ParamArg};
///
/// pub struct MyCrc;
///
/// impl FirmionExtension for MyCrc {
///     fn name(&self) -> &str { "my_org::crc" }
///     fn size(&self) -> usize { 4 }
///     fn execute<'a>(&self, args: &[ParamArg<'a>], out: &mut [u8]) -> Result<(), String> {
///         let val = match args.first() {
///             Some(ParamArg::Int(v)) => *v as u32,
///             _ => return Err("Expected one numeric argument".to_string()),
///         };
///         out.copy_from_slice(&val.to_be_bytes());
///         Ok(())
///     }
/// }
/// ```
///
/// # Example: section argument
///
/// ```rust
/// use firmion_extension::{FirmionExtension, ParamArg};
///
/// pub struct MyChecksum;
///
/// impl FirmionExtension for MyChecksum {
///     fn name(&self) -> &str { "my_org::checksum" }
///     fn size(&self) -> usize { 8 }
///     fn execute<'a>(&self, args: &[ParamArg<'a>], out: &mut [u8]) -> Result<(), String> {
///         let data = match args.first() {
///             Some(ParamArg::Slice { data }) => *data,
///             _ => return Err("Expected a slice argument".to_string()),
///         };
///         let sum: u64 = data.iter().map(|&b| b as u64).sum();
///         out.copy_from_slice(&sum.to_le_bytes());
///         Ok(())
///     }
/// }
/// ```
pub trait FirmionExtension {
    /// Returns the namespace-qualified name used to invoke the extension.
    /// For example, `"my_org::crc"`. The `firmion` and `std` namespaces are
    /// reserved for internal use.
    fn name(&self) -> &str;

    /// Returns the exact number of bytes the extension writes to `out`.
    ///
    /// Firmion calls this method once at registration and caches the result.
    /// All layout calculations use the cached value.
    fn size(&self) -> usize;

    /// Returns the declared parameters for this extension.
    ///
    /// Each entry in the slice declares one parameter: its name and kind.
    /// Returns the declared parameters for this extension.
    ///
    /// Each entry declares one parameter: its name and kind.  Firmion validates
    /// argument names against this slice and reorders call-site args to
    /// declaration order before passing them to [`execute`](Self::execute).
    fn params(&self) -> &[ParamDesc] {
        &[]
    }

    /// Produces the extension output bytes.
    ///
    /// * `args` -- one [`ExtArg`] per Firmion call-site argument, in declaration order.
    /// * `out`  -- pre-allocated buffer of exactly [`size`](Self::size) bytes.
    ///
    /// Return `Err(message)` to abort compilation with a diagnostic.
    fn execute<'a>(&self, args: &[ParamArg<'a>], out: &mut [u8]) -> Result<(), String>;
}
