// Output buffer abstraction for creating the raw binary image.
//
// OutputBuffer accumulates the binary image in a Vec<u8> during the exec
// phase.  Sequential append operations build the image in pass 1; random-
// access patch writes let extensions overwrite their pre-reserved zero-filled
// slots in pass 2.  The completed image is written to disk in a single call.
//

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use std::fs::File;
use std::io::{self, Read, Write};

pub struct OutputBuffer {
    data: Vec<u8>,
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Appends bytes to the buffer.  Infallible.
    pub fn append(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }

    /// Appends count zero bytes.  Infallible.
    /// Reserves space for extension output slots before pass 2 patches them.
    pub fn append_zeros(&mut self, count: usize) {
        self.data.resize(self.data.len() + count, 0);
    }

    /// Reads exactly byte_count bytes from source and appends them.
    /// Caller seeks source to the correct file position before calling.
    pub fn append_from_file(&mut self, source: &mut File, byte_count: u64) -> io::Result<()> {
        let start = self.data.len();
        self.data.resize(start + byte_count as usize, 0);
        source.read_exact(&mut self.data[start..])?;
        Ok(())
    }

    /// Returns a slice of the buffer at [start..end].
    /// Used to build ParamArg::Slice values for extension Slice params.
    pub fn slice(&self, start: usize, end: usize) -> &[u8] {
        &self.data[start..end]
    }

    /// Overwrites the bytes at offset with data.
    /// Used by execute_extensions to patch extension output into the image.
    /// Panics if offset + data.len() exceeds len(): the caller guarantees
    /// the slot was pre-reserved, so out-of-bounds is a compiler bug.
    pub fn patch(&mut self, offset: usize, data: &[u8]) {
        self.data[offset..offset + data.len()].copy_from_slice(data);
    }

    /// Returns the current byte count.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns true when the buffer holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Writes the entire buffer to file in one call.
    pub fn write_to_file(&self, file: &mut File) -> io::Result<()> {
        file.write_all(&self.data)
    }
}
