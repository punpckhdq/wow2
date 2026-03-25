use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::Result;

const LIBRARY_MAPPINGS: &[(&'static str, &'static str)] = &[
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\xapilib.lib",  "xapilib"),
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\d3d8.lib",     "d3d8"),
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\d3dx8.lib",    "d3dx8"),
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\dsound.lib",   "dsound"),
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\dsstrmh.lib",  "dsstrmh"),
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\xnet.lib",     "xnet"),
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\xkbd.lib",     "xkbd"),
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\xboxkrnl.lib", "xboxkrnl"),
    ("c:\\binkxbox\\binkxbox.lib",                                     "binkxbox"),
    ("C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\LIBCMT.lib",   "libcmt"),
];

const MODULE_BLACKLIST: &[&'static str] = &[
    "xboxkrnl.exe"
];

const LIBRARY_BLACKLIST: &[&'static str] = &[
    "C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\xbdm.lib",
    "C:\\Program Files\\Microsoft Xbox SDK\\Xbox\\Lib\\OLDNAMES.lib"
];

struct Section {
    name: &'static str,
    offset: usize,
    size: usize,
    flags: u32
}

const SECTIONS: &[Section] = &[
    Section { name: ".text",    offset: 0x000600, size: 0x1D5EC0, flags: 0x60000020 },
    Section { name: "D3D",      offset: 0x1D64C0, size: 0x017FC0, flags: 0xE0000020 },
    Section { name: "D3DX",     offset: 0x1EE480, size: 0x004D20, flags: 0xE0000020 },
    Section { name: "DSOUND",   offset: 0x1F31A0, size: 0x01F7A0, flags: 0xE0000020 },
    Section { name: "XNET",     offset: 0x212940, size: 0x00B0C0, flags: 0x60000020 },
    Section { name: "BINK",     offset: 0x21DA00, size: 0x016040, flags: 0x60000020 },
    Section { name: "BINK32",   offset: 0x233A40, size: 0x001280, flags: 0x60000020 },
    Section { name: "BINK32A",  offset: 0x234CC0, size: 0x001580, flags: 0x60000020 },
    Section { name: "BINK16",   offset: 0x236240, size: 0x001320, flags: 0x60000020 },
    Section { name: "BINK4444", offset: 0x237560, size: 0x0015A0, flags: 0x60000020 },
    Section { name: "BINK5551", offset: 0x238B00, size: 0x001140, flags: 0x60000020 },
    Section { name: "BINK16MX", offset: 0x239C40, size: 0x000140, flags: 0x60000020 },
    Section { name: "BINK16X2", offset: 0x239D80, size: 0x000560, flags: 0x60000020 },
    Section { name: "BINK16M",  offset: 0x23A2E0, size: 0x000200, flags: 0x60000020 },
    Section { name: "BINK32MX", offset: 0x23A4E0, size: 0x0001C0, flags: 0x60000020 },
    Section { name: "BINK32X2", offset: 0x23A6A0, size: 0x0005A0, flags: 0x60000020 },
    Section { name: "BINK32M",  offset: 0x23AC40, size: 0x000160, flags: 0x60000020 },
    Section { name: "XPP",      offset: 0x23ADA0, size: 0x007E80, flags: 0xE0000020 },
    Section { name: ".rdata",   offset: 0x242C20, size: 0x073E60, flags: 0x40000040 },
    Section { name: ".data",    offset: 0x2B6A80, size: 0x364180, flags: 0xC0000040 },
    Section { name: "DOLBY",    offset: 0x61AC00, size: 0x006DA0, flags: 0x40000040 },
    Section { name: "BINKDATA", offset: 0x6219A0, size: 0x004260, flags: 0xC0000040 },
    Section { name: "INIT",     offset: 0x625C00, size: 0x000300, flags: 0xE2000020 },
    Section { name: ".tls",     offset: 0x625F00, size: 0x000020, flags: 0xC2000040 },
    Section { name: ".XBLD",    offset: 0x625F20, size: 0x0000A0, flags: 0xC2000040 },
    Section { name: ".reloc",   offset: 0x625FC0, size: 0x0219A0, flags: 0x42000040 },
];

const LOAD_ADDRESS: usize = 0x00400000;

fn get_object_path(module_name: &str, obj_file: &str) -> Option<Box<str>> {
    if MODULE_BLACKLIST.iter().any(|b| *b == module_name) {
        return None;
    }

    if LIBRARY_BLACKLIST.iter().any(|b| *b == obj_file) {
        return None;
    }

    let file_name = if module_name == "* Linker *" {
        "linker_common.obj"
    } else {
        module_name.rsplit_once(&['\\', '/']).unwrap().1
    };

    let library_name = if obj_file.is_empty() || module_name == obj_file {
        "halobetacache"
    } else {
        LIBRARY_MAPPINGS.iter().find(|m| m.0 == obj_file).map(|m| m.1).unwrap()
    };

    Some(format!("{}/{}", library_name, file_name).into_boxed_str())
}

fn get_file_offset(section: usize, offset: usize) -> usize {
    let section = &SECTIONS[section];
    assert!(offset < section.size);
    section.offset + offset
}

fn get_default_symbol(file_offset: usize, flags: u32) -> Box<str> {
    let virtual_address = LOAD_ADDRESS + file_offset;
    format!("_{}_{:08x}",
        if flags & 0x20 > 0 {
            "code"
        } else if flags & 0x40 > 0 {
            if flags & 0x80000000 > 0 {
                "data"
            } else {
                "rdata"
            }
        } else if flags & 0x80 > 0 {
            "bss"
        } else {
            "unk"
        }, file_offset).into_boxed_str()
}

fn main() -> Result<()> {
    let pdb = ms_pdb::Pdb::open(Path::new("cachebeta.pdb"))?;

    let mut symbols_csv = File::create("symbols.csv")?;

    let mut defined_symbols = Vec::new();

    let gss = pdb.gss()?;
    for sym in gss.iter_syms() {
        match sym.parse()? {
            ms_pdb::syms::SymData::Pub(data) => {
                let os = data.fixed.offset_segment.as_tuple();
                if os.0 >= 27 {
                    continue;
                }
                let offset = get_file_offset(os.0 as usize - 1, os.1 as usize);
                write!(&mut symbols_csv, "{},{}\n",
                    offset,
                    data.name)?;
                defined_symbols.push(offset);
            },
            _ => {}
        }
    }

    defined_symbols.sort();

    let mut modules_csv = File::create("modules.csv")?;

    let dbi = pdb.read_dbi_stream()?;
    for (i, module) in dbi.iter_modules().enumerate() {
        if let Some(obj_path) = get_object_path(
                &module.module_name().to_string(),
                &module.obj_file().to_string()) {
            write!(&mut modules_csv, "{},{}\n",
                i,
                obj_path)?;
        }
    }

    let mut contribs_csv = File::create("contribs.csv")?;

    let mut splits = vec![Vec::<usize>::new(); SECTIONS.len()];
    let mut last_index = None;
    let mut last_section = None;

    for contrib in dbi.section_contributions()?.contribs {
        let section_index = contrib.section.get() as usize - 1;
        let offset = contrib.offset.get() as usize;
        let module_index = contrib.module_index.get();

        if last_index.is_none_or(|j| j < module_index) ||
            last_section.is_none_or(|j| j != section_index) {
            splits[section_index].push(offset);
        }

        last_index = Some(module_index);
        last_section = Some(section_index);

        let file_offset = get_file_offset(section_index, offset);
        let characteristics = contrib.characteristics.get();
        if defined_symbols.binary_search(&file_offset).is_err() {
            // Give unnamed contribs a default symbol
            write!(&mut symbols_csv, "{},{}\n",
                file_offset,
                get_default_symbol(file_offset, characteristics))?;
        }

        write!(&mut contribs_csv, "{},{},{},{},{},{}\n",
            file_offset,
            contrib.size.get(),
            characteristics,
            module_index,
            contrib.data_crc.get(),
            contrib.reloc_crc.get())?;
    }

    let mut splits_csv = File::create("splits.csv")?;

    for (section_index, splits) in splits.iter().enumerate() {
        let section = &SECTIONS[section_index];

        if splits.len() == 0 {
            write!(&mut splits_csv, "{},{},{},{}\n",
                section.offset,
                section.size,
                section.name,
                section.flags)?;
        } else {
            for i in 0..splits.len() {
                let split = &splits[i];
                let size = splits.get(i + 1)
                    .map_or(section.size - split, |s| s - split);
                write!(&mut splits_csv, "{},{},{},{}\n",
                    section.offset + split,
                    size,
                    if i > 0 {
                        &format!("{}${}", section.name, i).into_boxed_str()
                    } else {
                        section.name
                    },
                    section.flags)?;
            }
        }
    }

    Ok(())
}
