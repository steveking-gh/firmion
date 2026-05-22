// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use ir::ParameterValue;

#[derive(Clone, Debug)]
pub struct SectionEntry {
    pub name: String,
    pub file_offset: u64,
    pub off: u64,
    pub abs_start: u64,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct LabelEntry {
    pub name: String,
    pub file_offset: u64,
    pub off: u64,
    pub abs_addr: u64,
}

#[derive(Clone, Debug)]
pub struct ConstEntry {
    pub name: String,
    pub value: ParameterValue,
    pub used: bool,
}

#[derive(Clone, Debug)]
pub struct MapDb {
    pub output_file: String,
    pub base_addr: u64,
    pub total_size: u64,
    pub sections: Vec<SectionEntry>,
    pub labels: Vec<LabelEntry>,
    pub consts: Vec<ConstEntry>,
}
