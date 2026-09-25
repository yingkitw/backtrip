//! JVM bytecode decoder: `&[u8]` → `Vec<Instruction>` (JVMS 6.5).
//!
//! Branch operands are stored as absolute code offsets (JVM branch offsets
//! are relative to the *opcode address*). `wide` (0xC4) is expanded into the
//! modified instruction it prefixes.

use crate::error::{Error, Result};
use crate::java::opcodes::{lookup, Kind};

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    None,
    I8(i8),
    I16(i16),
    I32(i32),
    /// Constant pool index.
    Cp(u16),
    /// Local variable slot (u16 — `wide`-prefixed forms index past 255).
    Local(u16),
    /// `iinc` — slot + signed delta.
    Iinc(u8, i8),
    /// Wide `iinc` — slot + 16-bit delta.
    WideIinc(u16, i16),
    /// `newarray` primitive type code.
    AType(u8),
    /// `multianewarray` — Cp index + dimensions.
    Dims(u16, u8),
    /// Absolute branch target.
    Target(usize),
    TableSwitch {
        default: usize,
        low: i32,
        high: i32,
        /// Absolute targets for values low..=high.
        targets: Vec<usize>,
    },
    LookupSwitch {
        default: usize,
        /// (match value, absolute target) pairs, sorted by value.
        pairs: Vec<(i32, usize)>,
    },
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub offset: usize,
    pub op: u8,
    pub name: &'static str,
    pub operand: Operand,
    pub size: usize,
    /// True when this instruction was prefixed by `wide`.
    pub wide: bool,
}

pub fn decode(code: &[u8]) -> Result<Vec<Instruction>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < code.len() {
        let start = p;
        let mut b = *code.get(p).ok_or_else(|| Error::InvalidClassFile("truncated code".into()))?;
        p += 1;
        let mut wide = false;
        if b == 0xC4 {
            wide = true;
            b = *code.get(p).ok_or_else(|| Error::InvalidClassFile("truncated wide prefix".into()))?;
            p += 1;
            let info = lookup(b);
            // Wide modifies Local and Iinc forms only.
            let (operand, adv) = match info.kind {
                Kind::Local => {
                    need(code, p, 2)?;
                    (Operand::Local(u16::from_be_bytes([code[p], code[p + 1]])), 2)
                }
                Kind::Iinc => {
                    need(code, p, 4)?;
                    let idx = u16::from_be_bytes([code[p], code[p + 1]]);
                    let delta = i16::from_be_bytes([code[p + 2], code[p + 3]]);
                    (Operand::WideIinc(idx, delta), 4)
                }
                _ => {
                    return Err(Error::InvalidClassFile(format!(
                        "wide prefix on non-local opcode {b:#04X}"
                    )))
                }
            };
            p += adv;
            out.push(Instruction {
                offset: start,
                op: b,
                name: info.name,
                operand,
                size: p - start,
                wide,
            });
            continue;
        }

        let info = lookup(b);
        let (operand, adv) = read_operand(code, p, start, info.kind)?;
        p += adv;
        out.push(Instruction {
            offset: start,
            op: b,
            name: info.name,
            operand,
            size: p - start,
            wide,
        });
    }
    Ok(out)
}

fn read_operand(code: &[u8], p: usize, start: usize, kind: Kind) -> Result<(Operand, usize)> {
    use Kind::*;
    Ok(match kind {
        None => (Operand::None, 0),
        SByte => (Operand::I8(read_i8(code, p)?), 1),
        SShort => (Operand::I16(read_i16(code, p)?), 2),
        Cp1 => (Operand::Cp(read_u8(code, p)? as u16), 1),
        Cp2 => (Operand::Cp(read_u16(code, p)?), 2),
        Cp2Extra1 => (Operand::Cp(read_u16(code, p)?), 4), // count + zero skipped
        Cp2Extra2 => (Operand::Cp(read_u16(code, p)?), 4), // 2 zero bytes skipped
        Local => (Operand::Local(read_u8(code, p)?.into()), 1),
        Iinc => (Operand::Iinc(read_u8(code, p)?, read_i8(code, p + 1)?), 2),
        AType => (Operand::AType(read_u8(code, p)?), 1),
        Dims => (Operand::Dims(read_u16(code, p)?, read_u8(code, p + 2)?), 3),
        Branch => {
            let delta = read_i16(code, p)? as i32;
            (Operand::Target((start as i64 + delta as i64).max(0) as usize), 2)
        }
        BranchW => {
            need(code, p, 4)?;
            let delta = i32::from_be_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]]);
            (Operand::Target((start as i64 + delta as i64).max(0) as usize), 4)
        }
        TableSwitch => {
            // Padding to 4-byte alignment relative to the code start.
            let pad = (4 - (p % 4)) % 4;
            let base = p + pad;
            need(code, base, 12)?;
            let default = (start as i64 + read_i32(code, base)? as i64) as usize;
            let low = read_i32(code, base + 4)?;
            let high = read_i32(code, base + 8)?;
            if high < low {
                return Err(Error::InvalidClassFile(format!(
                    "tableswitch high {high} < low {low}"
                )));
            }
            let n = (high - low + 1) as usize;
            need(code, base + 12, n * 4)?;
            let mut targets = Vec::with_capacity(n);
            for i in 0..n {
                let off = read_i32(code, base + 12 + i * 4)?;
                targets.push((start as i64 + off as i64) as usize);
            }
            (Operand::TableSwitch { default, low, high, targets }, pad + 12 + n * 4)
        }
        LookupSwitch => {
            let pad = (4 - (p % 4)) % 4;
            let base = p + pad;
            need(code, base, 8)?;
            let default = (start as i64 + read_i32(code, base)? as i64) as usize;
            let n = read_i32(code, base + 4)? as usize;
            need(code, base + 8, n * 8)?;
            let mut pairs = Vec::with_capacity(n);
            for i in 0..n {
                let v = read_i32(code, base + 8 + i * 8)?;
                let off = read_i32(code, base + 12 + i * 8)?;
                pairs.push((v, (start as i64 + off as i64) as usize));
            }
            (Operand::LookupSwitch { default, pairs }, pad + 8 + n * 8)
        }
    })
}

fn need(code: &[u8], p: usize, n: usize) -> Result<()> {
    if p + n > code.len() {
        return Err(Error::InvalidClassFile(format!(
            "operand needs {n} bytes at code offset {p}"
        )));
    }
    Ok(())
}

fn read_u8(code: &[u8], p: usize) -> Result<u8> {
    need(code, p, 1)?;
    Ok(code[p])
}

fn read_i8(code: &[u8], p: usize) -> Result<i8> {
    Ok(read_u8(code, p)? as i8)
}

fn read_u16(code: &[u8], p: usize) -> Result<u16> {
    need(code, p, 2)?;
    Ok(u16::from_be_bytes([code[p], code[p + 1]]))
}

fn read_i16(code: &[u8], p: usize) -> Result<i16> {
    need(code, p, 2)?;
    Ok(i16::from_be_bytes([code[p], code[p + 1]]))
}

fn read_i32(code: &[u8], p: usize) -> Result<i32> {
    need(code, p, 4)?;
    Ok(i32::from_be_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instrs(code: &[u8]) -> Vec<Instruction> {
        decode(code).unwrap()
    }

    #[test]
    fn decodes_simple_sequence() {
        // iconst_5, istore_1, iload_0, ireturn
        let is = instrs(&[0x08, 0x3C, 0x1A, 0xAC]);
        assert_eq!(is.len(), 4);
        assert_eq!(is[0].name, "iconst_5");
        assert_eq!(is[1].name, "istore_1");
        assert_eq!(is[2].name, "iload_0");
        assert_eq!(is[3].name, "ireturn");
        assert_eq!(is[3].offset, 3);
    }

    #[test]
    fn branch_targets_are_absolute() {
        // 0: iconst_1        (1 byte)
        // 1: ifne +3         → target 1+3 = 4
        // 3: nop
        // 4: return
        let is = instrs(&[0x04, 0x9A, 0x00, 0x03, 0x00, 0xB1]);
        assert_eq!(is[1].name, "ifne");
        assert_eq!(is[1].operand, Operand::Target(4));
    }

    #[test]
    fn decodes_iinc_and_wide() {
        // iinc 0 +1 ; wide iinc 300 +1000
        let mut code = vec![0x84, 0x00, 0x01, 0xC4, 0x84];
        code.extend_from_slice(&0x012C_u16.to_be_bytes());
        code.extend_from_slice(&1000_i16.to_be_bytes());
        let is = instrs(&code);
        assert_eq!(is[0].operand, Operand::Iinc(0, 1));
        assert!(is[1].wide);
        assert_eq!(is[1].name, "iinc");
        assert_eq!(is[1].operand, Operand::WideIinc(300, 1000));
    }

    #[test]
    fn decodes_tableswitch_with_padding() {
        // 0: tableswitch, offsets start at byte 1 → pad 3 → data at 4
        // default = +10 → 10; low=1, high=2; targets +20, +30
        let mut code = vec![0xAA, 0x00, 0x00, 0x00];
        code.extend_from_slice(&10_i32.to_be_bytes());
        code.extend_from_slice(&1_i32.to_be_bytes());
        code.extend_from_slice(&2_i32.to_be_bytes());
        code.extend_from_slice(&20_i32.to_be_bytes());
        code.extend_from_slice(&30_i32.to_be_bytes());
        let is = instrs(&code);
        match &is[0].operand {
            Operand::TableSwitch { default, low, high, targets } => {
                assert_eq!(*default, 10);
                assert_eq!(*low, 1);
                assert_eq!(*high, 2);
                assert_eq!(*targets, vec![20, 30]);
            }
            other => panic!("expected tableswitch, got {other:?}"),
        }
        assert_eq!(is[0].size, 1 + 3 + 12 + 8);
    }

    #[test]
    fn decodes_lookupswitch() {
        let mut code = vec![0xAB, 0x00, 0x00, 0x00];
        code.extend_from_slice(&5_i32.to_be_bytes()); // default +5 → 5
        code.extend_from_slice(&2_i32.to_be_bytes()); // 2 pairs
        code.extend_from_slice(&7_i32.to_be_bytes());
        code.extend_from_slice(&40_i32.to_be_bytes());
        code.extend_from_slice(&9_i32.to_be_bytes());
        code.extend_from_slice(&60_i32.to_be_bytes());
        let is = instrs(&code);
        match &is[0].operand {
            Operand::LookupSwitch { default, pairs } => {
                assert_eq!(*default, 5);
                assert_eq!(*pairs, vec![(7, 40), (9, 60)]);
            }
            other => panic!("expected lookupswitch, got {other:?}"),
        }
    }

    #[test]
    fn decodes_invokeinterface_size() {
        // invokevirtual #4 (3 bytes) then invokeinterface #4 count 1 (5 bytes)
        let code = [0xB6, 0x00, 0x04, 0xB9, 0x00, 0x04, 0x01, 0x00];
        let is = instrs(&code);
        assert_eq!(is[0].size, 3);
        assert_eq!(is[1].size, 5);
        assert_eq!(is[1].operand, Operand::Cp(4));
    }

    #[test]
    fn truncation_is_an_error() {
        assert!(decode(&[0x11, 0x00]).is_err()); // sipush missing byte
    }
}
