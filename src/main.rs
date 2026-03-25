use std::collections::{VecDeque, HashMap};
use std::io::{Cursor, Read};

mod data;
mod table;

use data::{Contrib, Module, Split, Symbol};
use table::{Imm, Opcode};

/// Friendly wrapper around [`Error`]
type Result<T> = std::result::Result<T, Error>;

/// Error types
#[derive(Debug)]
enum Error {
    /// Failed to open the EXE file
    Open(std::io::Error),

    /// Reading instruction bytes failed
    ReadInst(std::io::Error),

    /// Reading jump table bytes failed
    ReadJumpTable(std::io::Error),

    /// Reading relocation data failed
    ReadReloc(std::io::Error),

    /// Reading relocation target failed
    ReadRelocTarget,

    /// Integer overflow when computing next position
    IntegerOverflowPosition,

    /// Integer overflow when computing jump target
    IntegerOverflowJump,

    /// Integer overflow when computing call target
    IntegerOverflowCall,

    /// Integer overflow when computing jump table offset
    IntegerOverflowJumpTable,

    /// Integer overflow when computing reloc target offset
    IntegerOverflowRelocTarget,

    /// Integer overflow when computing nearest symbol
    IntegerOverflowNearestSymbol,

    /// Integer overflow when computing nearest contrib
    IntegerOverflowNearestContrib,

    /// An invalid opcode was encountered
    InvalidOpcode(u8),

    /// An unrelocatable jump was encountered
    InvalidJump,

    /// An unrelocatable call was encountered
    InvalidCall,
}

/// Read SIB if required by ModRM byte
macro_rules! get_sib {
    ($mod:expr, $rm:expr, $inst:ident, $ofs:ident) => {{
        if $mod != 3 && $rm == 4 {
            let sib = $inst[$ofs];
            $ofs += 1;
            Some(sib)
        } else { None }
    }};
}

/// Get ModRM displacement size
macro_rules! get_disp_size {
    ($mod:expr, $rm:expr, $sib:expr) => {{
        match ($mod, $rm) {
            (0, 5) => 4,
            (0, _) => if $sib
                .is_some_and(|s| s & 0x7 == 5) {
                    4
                } else {
                    0
                },
            (1, _) => 1,
            (2, _) => 4,
            _ => 0,
        }
    }};
}

/// Unsafely transmute slice to type
macro_rules! cast_slice {
    ($slice:expr, $ty:ty) => {{
        std::ptr::read_unaligned($slice.as_ptr() as *const $ty)
    }}
}

/// Helper function to get nearest symbol for a file offset
fn nearest_symbol(file_offset: usize) -> Result<(usize, usize)> {
    let index = data::SYMBOLS
        .partition_point(|s| s.file_offset <= file_offset)
        .checked_sub(1).ok_or(Error::IntegerOverflowNearestSymbol)?;
    let offset = file_offset - &data::SYMBOLS[index].file_offset;
    Ok((index, offset))
}

/// Helper function to get nearest contrib for a file offset
fn nearest_contrib(file_offset: usize) -> Result<(usize, usize)> {
    let index = data::CONTRIBS
        .partition_point(|c| c.file_offset < file_offset)
        .checked_sub(1).ok_or(Error::IntegerOverflowNearestContrib)?;
    let offset = file_offset - &data::CONTRIBS[index].file_offset;
    Ok((index, offset))
}

/// A reference from one address to another
#[derive(Debug)]
struct Reloc {
    /// File offset of the pointer
    source_file_offset: usize,

    /// File offset of the pointer's value
    target_file_offset: usize,

    /// Negative addend applied to the pointer value. If above
    /// zero, this relocation is relative to `source_file_offset`.
    target_addend: usize,
}

/// Locate 32-bit relative offset operands
fn analyze_function(
    source_index: usize,
    file_offset: usize,
    func_bytes: &[u8],
    relocs: &mut Vec<Reloc>) -> Result<()> {
    let mut reader = Cursor::new(func_bytes);
    let func_bounds = 0..reader.get_ref().len() as u64;

    let mut queue = VecDeque::from([0]);
    let mut visited = vec![false; func_bytes.len()];

    // Go through each block
    while let Some(cur) = queue.pop_front() {
        reader.set_position(cur);
        //println!("-> {:04x}", cur);
        loop {
            let pos = reader.position();

            // If there's no more data, break
            if !func_bounds.contains(&pos) {
                //println!("<function ended>");
                break;
            }

            // Skip if already visited
            let has_visited = &mut visited[pos as usize];
            if *has_visited {
                break;
            } else {
                *has_visited = true;
            }

            // Get potential bytes for this instruction
            let mut inst = [0; 16];
            reader.read(&mut inst[..]).map_err(|x| Error::ReadInst(x))?;

            let mut ofs = 0;

            let mut opcode_sse = false;
            let mut op_size = false;
            let mut addr_size = false;

            // Read prefixes
            for _ in 0..4 {
                let prefix = inst[ofs];
                match prefix {
                    0x0f => { opcode_sse = true; },
                    0x2e | 0x36 | 0x3e | 0x26 | 0x64 | 0x65 => {},
                    0x66 => { op_size = true; },
                    0x67 => { addr_size = true; },
                    0xf0 | 0xf2 | 0xf3 => {},
                    _ => { break; },
                }
                ofs += 1;

                // Break on `opcode_sse` as an opcode byte
                if opcode_sse {
                    break;
                }
            }

            // Decode instruction
            let opcode = inst[ofs];
            //println!("{:04x} {:02x}", pos, opcode);
            ofs += 1;
            let ends_block = match (opcode_sse, opcode) {
                (false, 0x70..=0x7f | 0xeb) => {
                    // Parse Jcc, Jb and JMP short Jb
                    let imm = inst[ofs] as i8 as i64;
                    ofs += 1;
                    let jump_target = pos.checked_add(ofs as u64)
                        .and_then(|p| p.checked_add_signed(imm))
                        .ok_or(Error::IntegerOverflowJump)?;
                    if !func_bounds.contains(&jump_target) {
                        // Shouldn't ever happen
                        return Err(Error::InvalidJump);
                    }
                    queue.push_back(jump_target);

                    // JMP is unconditional
                    opcode == 0xeb
                },
                (false, 0xc2 | 0xc3 | 0xcc) => {
                    // End block on RET, INT3
                    true
                },
                (false, 0xe8) => {
                    // Parse CALL Jv
                    if op_size {
                        // Shouldn't ever happen
                        return Err(Error::InvalidCall);
                    }

                    // SAFETY: `ofs` cannot exceed 12 bytes
                    let imm = unsafe { cast_slice!(&inst[ofs..], i32) };
                    ofs += 4;

                    let file_eip_offset = file_offset + pos as usize + ofs;
                    let call_file_offset = file_eip_offset
                        .checked_add_signed(imm as isize)
                        .ok_or(Error::IntegerOverflowCall)?;
                    relocs.push(Reloc {
                        source_file_offset: file_eip_offset - 4,
                        target_file_offset: call_file_offset,
                        target_addend: ofs,
                    });

                    false
                },
                (false, 0xe9) | (true, 0x80..=0x8f) => {
                    // Parse JMP near Jv, Jcc Jv
                    // SAFETY: `ofs` cannot exceed 12 bytes
                    let imm = if op_size {
                        let imm = unsafe { cast_slice!(&inst[ofs..], i16) };
                        ofs += 2;
                        imm as isize
                    } else {
                        let imm = unsafe { cast_slice!(&inst[ofs..], i32) };
                        ofs += 4;
                        imm as isize
                    };

                    let file_eip_offset = file_offset + pos as usize + ofs;
                    let jump_file_offset = file_eip_offset
                        .checked_add_signed(imm)
                        .ok_or(Error::IntegerOverflowJump)?;
                    if let Some(jump_target) = jump_file_offset
                        .checked_sub(file_offset).map(|j| j as u64)
                        && func_bounds.contains(&jump_target) {
                        queue.push_back(jump_target);
                    } else {
                        if op_size {
                            // Shouldn't ever happen
                            return Err(Error::InvalidJump);
                        }
                        relocs.push(Reloc {
                            source_file_offset: file_eip_offset - 4,
                            target_file_offset: jump_file_offset,
                            target_addend: ofs,
                        });
                    }

                    // JMP is unconditional
                    opcode == 0xe9
                },
                (false, 0xf6 | 0xf7) => {
                    // Parse Group 3 opcodes
                    let modrm = inst[ofs];
                    ofs += 1;

                    // Skip displacement
                    let mode = modrm >> 6;
                    let rm   = modrm & 0x7;
                    let sib  = get_sib!(mode, rm, inst, ofs);
                    ofs += get_disp_size!(mode, rm, sib);

                    // Skip immediate
                    let reg = (modrm >> 3) & 0x7;
                    if reg == 0 {
                        let imm_size = if opcode == 0xf7 {
                            if op_size { 2 } else { 4 }
                        } else { 1 };
                        ofs += imm_size;
                    }

                    false
                },
                (false, 0xff) => {
                    // Parse Group 5 opcode
                    let modrm = inst[ofs];
                    ofs += 1;

                    // Get displacement size
                    let mode = modrm >> 6;
                    let reg  = (modrm >> 3) & 0x7;
                    let rm   = modrm & 0x7;
                    let sib  = get_sib!(mode, rm, inst, ofs);
                    let disp_size = get_disp_size!(mode, rm, sib);

                    if let Some(sib) = sib
                        && sib >> 6 == 2
                        && disp_size == 4
                        && reg == 4 {
                        // Jump table through JMPN Ev
                        // SAFETY: `ofs` cannot exceed 12 bytes
                        let disp = unsafe { cast_slice!(&inst[ofs..], u32) };
                        ofs += 4;

                        // Make displacement relative
                        let disp_file_offset =
                            disp.checked_sub(data::LOAD_ADDRESS)
                            .ok_or(Error::IntegerOverflowJumpTable)?
                            as usize;

                        //println!("JMP TABLE {:08x}", disp_file_offset);

                        // Check if table belongs to us
                        if let Some(disp_target) = disp_file_offset
                            .checked_sub(file_offset).map(|d| d as u64)
                            && func_bounds.contains(&disp_target) {
                            // Find jump cases
                            let mut reader = reader.clone();
                            reader.set_position(disp_target);
                            loop {
                                // Read next pointer
                                let mut raw = [0u8; 4];
                                let bytes_read = reader.read(&mut raw[..])
                                    .map_err(|x| Error::ReadJumpTable(x))?;

                                // If there's no more data, break
                                if bytes_read != 4 {
                                    break;
                                }

                                // Check if `ptr` is a valid jump target
                                let ptr = u32::from_le_bytes(raw);
                                if let Some(ptr_target) = ptr
                                    .checked_sub(data::LOAD_ADDRESS)
                                    .and_then(|j| (j as usize)
                                        .checked_sub(file_offset))
                                    .map(|j| j as u64)
                                    && func_bounds.contains(&ptr_target) {
                                        //println!("Queued {:04x}", ptr_target);
                                        queue.push_back(ptr_target);
                                } else {
                                    break;
                                }
                            }
                        }
                    } else {
                        // Don't care
                        ofs += disp_size;
                    }

                    // JMPN, JMPF are unconditional
                    reg == 4 || reg == 5
                },
                (true, 0x71..0x74 | 0xd1..0xd4 | 0xe1..0xe3 | 0xf1..0xf4) => {
                    // Parse Group 12, 13, 14 opcodes
                    let modrm = inst[ofs];
                    ofs += 1;

                    // Skip displacement
                    let mode = modrm >> 6;
                    let rm   = modrm & 0x7;
                    let sib  = get_sib!(mode, rm, inst, ofs);
                    ofs += get_disp_size!(mode, rm, sib);

                    // Skip immediate
                    let reg = (modrm >> 3) & 0x7;
                    if reg == 2 || reg == 6 || (opcode & 0xf != 3 && reg == 4) {
                        ofs += 1;
                    }

                    false
                },
                _ => {
                    // Fall back to opcode table. This is for opcodes we don't
                    // care about so it only needs to read enough to skip
                    let table = if opcode_sse { table::OPCODE_TABLE_0F }
                    else { table::OPCODE_TABLE };
                    match &table[opcode as usize] {
                        Opcode::Inst { has_modrm, immediate } => {
                            // Skip displacement
                            ofs += if *has_modrm {
                                let modrm = inst[ofs];
                                ofs += 1;
                                let mode = modrm >> 6;
                                let rm   = modrm & 0x7;
                                let sib  = get_sib!(mode, rm, inst, ofs);
                                get_disp_size!(mode, rm, sib)
                            } else { 0 };

                            // Skip immediate
                            ofs += match *immediate {
                                Imm::Constant(size) => size,
                                Imm::OpSize => if op_size { 2 } else { 4 },
                                Imm::AddrSize => if addr_size { 2 } else { 4 },
                            };
                        },
                        _ => { return Err(Error::InvalidOpcode(opcode)) },
                    }

                    false
                },
            };

            // Break on end of block
            if ends_block {
                //println!("<end of block>");
                break;
            }

            // Seek to next instruction
            let pos = pos.checked_add(ofs as u64)
                .ok_or(Error::IntegerOverflowPosition)?;
            reader.set_position(pos);
        }
    }

    Ok(())
}

/// Parses a .reloc section and pushes relocations to `relocs`
fn read_reloc_table(bytes: &[u8], split: &Split, relocs: &mut Vec<Reloc>)
    -> Result<()> {
    let reloc_bytes =
        &bytes[split.file_offset..split.file_offset + split.size];
    let mut reader = Cursor::new(reloc_bytes);
    loop {
        let mut raw = [0u8; 4];
        reader.read_exact(&mut raw[..])
            .map_err(|x| Error::ReadReloc(x))?;
        let block_file_offset = u32::from_le_bytes(raw) as usize;
        reader.read_exact(&mut raw[..])
            .map_err(|x| Error::ReadReloc(x))?;

        let block_size = u32::from_le_bytes(raw) as u64;
        let Some(size) = block_size.checked_sub(8) else { break; };
        let end = reader.position() + size;
        for _ in 0..(size / 2) {
            let mut raw = [0u8; 2];
            reader.read_exact(&mut raw[..])
                .map_err(|x| Error::ReadReloc(x))?;
            let offset = u16::from_le_bytes(raw) as usize;
            if offset == 0 { break; }

            let source_file_offset = block_file_offset + (offset & 0x0fff);
            let raw: [u8; 4] = bytes[source_file_offset..source_file_offset + 4]
                .try_into().map_err(|_| Error::ReadRelocTarget)?;
            let target_file_offset = u32::from_le_bytes(raw)
                .checked_sub(data::LOAD_ADDRESS)
                .ok_or(Error::IntegerOverflowRelocTarget)?
                as usize;
            relocs.push(Reloc {
                source_file_offset: source_file_offset,
                target_file_offset: target_file_offset,
                target_addend: 0,
            });
        }
        reader.set_position(end);
    }

    Ok(())
}

enum ObjectSymbol<'n> {
    Local {
        name: &'n str,
        contrib_index: usize,
        contrib_offset: usize,
    },

    Extern {
        name: &'n str,
    },
}

/// Defines the contents of an object file
///
/// An object file is comprised of different sections for each contribution
/// along with a global index of symbols.
struct ObjectLayout {
    contribs: Vec<usize>,
    symbols: Vec<ObjectSymbol>,
}

fn split_module(module_index: usize) -> Result<()> {
    Ok(())
}

/// Entry point
fn main() -> Result<()> {
    let bytes = std::fs::read("cachebeta.exe").map_err(|x| Error::Open(x))?;

    // First, find all relocs
    let mut relocs = Vec::new();
    read_reloc_table(&bytes, &data::SPLITS.last().unwrap(), &mut relocs)?;
    for (i, contrib) in data::CONTRIBS.iter().enumerate() {
        let func_bytes =
            &bytes[contrib.file_offset..contrib.file_offset + contrib.size];

        // If the contribution contains code, disassemble
        if contrib.characteristics & 0x20 != 0 {
            analyze_function(
                i,
                contrib.file_offset,
                func_bytes,
                &mut relocs)?;
        }
    }
    relocs.sort_by(|x, y| {
        x.source_file_offset.cmp(&y.source_file_offset)
    });

    //for reloc in relocs {
    //    println!("{:?}", reloc);
    //}

    for contrib in data::CONTRIBS {
        let range = contrib.file_offset..contrib.file_offset + contrib.size;
        let matches: Vec<&Symbol> = data::SYMBOLS.iter()
            .filter(|s| range.contains(&s.file_offset)).collect();
        if matches.len() > 1 {
            let a = if contrib.characteristics & 0x80 != 0 {
                "BSS"
            } else if contrib.characteristics & 0x20 != 0 {
                "TEXT"
            } else {
                "DATA"
            };

            let b = if contrib.characteristics & 0x1000 != 0 {
                " COMDAT"
            } else {
                ""
            };

            println!("{}{} {:?} {:?}", a, b, contrib, matches);
        }
    }

    Ok(())
}
