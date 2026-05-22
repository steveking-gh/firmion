// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use diags::{Diags, SourceSpan};

#[derive(Clone, Debug, PartialEq)]
pub struct AddressState {
    pub addr_offset: u64,
    pub sec_offset: u64,
    pub addr_base: u64,
}

impl AddressState {
    pub fn advance(&mut self, sz: u64) {
        self.sec_offset = self.sec_offset.saturating_add(sz);
        self.addr_offset += sz;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub file_offset: u64,
    pub addr: AddressState,
}

impl Location {
    pub fn advance(&mut self, size: u64, src_loc: &SourceSpan, diags: &mut Diags) -> bool {
        let Some(new_file_pos) = self.file_offset.checked_add(size) else {
            diags.err1(
                "ERR_159",
                "Write operation causes file offset overflow",
                src_loc.clone(),
            );
            return false;
        };
        let new_off = self.addr.addr_offset + size; // safe: off <= file_pos
        if self.addr.addr_base.checked_add(new_off).is_none() {
            diags.err1(
                "ERR_165",
                "Write operation causes absolute address overflow",
                src_loc.clone(),
            );
            return false;
        }
        self.file_offset = new_file_pos;
        self.addr.addr_offset = new_off;
        self.addr.sec_offset = self.addr.sec_offset.saturating_add(size);
        true
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "file offset {}, addr offset {}, sec offset {}",
            self.file_offset, self.addr.addr_offset, self.addr.sec_offset
        )
    }
}

pub struct LocationDb {
    pub ir_locs: Vec<Location>,
}
