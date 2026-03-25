#[derive(Debug)]
pub struct Module {
    pub index: usize,
    pub path: &'static str,
}

#[derive(Debug)]
pub struct Symbol {
    pub file_offset: usize,
    pub name: &'static str,
}

#[derive(Debug)]
pub struct Contrib {
    pub file_offset: usize,
    pub size: usize,
    pub characteristics: u32,
    pub module_index: usize,
}

#[derive(Debug)]
pub struct Split {
    pub file_offset: usize,
    pub size: usize,
    pub name: &'static str,
    pub flags: u32,
}

pub const MODULES: &[Module] = include!("modules.txt");
pub const SYMBOLS: &[Symbol] = include!("symbols.txt");
pub const CONTRIBS: &[Contrib] = include!("contribs.txt");
pub const SPLITS: &[Split] = include!("splits.txt");

pub const LOAD_ADDRESS: u32 = 0x00400000;
