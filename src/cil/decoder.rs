use crate::cil::opcodes::{lookup, OpInfo, OpKind};
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub enum Operand {
    None,
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    R4(f32),
    R8(f64),
    BrTarget(i32),       // relative offset (target = instr_end + offset)
    ShortBrTarget(i8),
    Switch(Vec<i32>),    // relative offsets
    Token(u32),
    ShortVar(u8),
    Var(u16),
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub offset: usize,
    pub op: u16,
    pub name: &'static str,
    pub operand: Operand,
    /// Byte length of the whole instruction (opcode + operand).
    pub size: usize,
}

impl Instruction {
    /// Absolute target offset for a branch instruction, if any.
    pub fn branch_target(&self) -> Option<usize> {
        let base = self.offset as i64 + self.size as i64;
        match &self.operand {
            Operand::BrTarget(o) => Some((base + *o as i64).max(0) as usize),
            Operand::ShortBrTarget(o) => Some((base + *o as i64).max(0) as usize),
            _ => None,
        }
    }
}

pub fn decode(code: &[u8]) -> Result<Vec<Instruction>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < code.len() {
        let start = p;
        let b0 = code[p];
        p += 1;
        let (op_val, info) = if b0 == 0xFE {
            if p >= code.len() {
                return Err(Error::InvalidCil("truncated two-byte opcode".into()));
            }
            let b1 = code[p];
            p += 1;
            let v = 0xFE00 | (b1 as u16);
            match lookup(v) {
                Some(i) => (v, i),
                None => (v, OpInfo { name: "unknown", kind: OpKind::None }),
            }
        } else {
            match lookup(b0 as u16) {
                Some(i) => (b0 as u16, i),
                None => (b0 as u16, OpInfo { name: "unknown", kind: OpKind::None }),
            }
        };

        let (operand, adv) = read_operand(code, p, info.kind)?;
        p += adv;
        out.push(Instruction {
            offset: start,
            op: op_val,
            name: info.name,
            operand,
            size: p - start,
        });
    }
    Ok(out)
}

fn read_operand(code: &[u8], p: usize, kind: OpKind) -> Result<(Operand, usize)> {
    use OpKind::*;
    Ok(match kind {
        None => (Operand::None, 0),
        SByte => {
            let v = (*code.get(p).ok_or_else(|| Error::InvalidCil("missing int8 operand".into()))?) as i8;
            (Operand::I8(v), 1)
        }
        Short => {
            need(code, p, 2)?;
            let v = i16::from_le_bytes([code[p], code[p + 1]]);
            (Operand::I16(v), 2)
        }
        Int => {
            need(code, p, 4)?;
            let v = i32::from_le_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]]);
            (Operand::I32(v), 4)
        }
        Long => {
            need(code, p, 8)?;
            let v = i64::from_le_bytes([
                code[p], code[p + 1], code[p + 2], code[p + 3],
                code[p + 4], code[p + 5], code[p + 6], code[p + 7],
            ]);
            (Operand::I64(v), 8)
        }
        Float => {
            need(code, p, 4)?;
            let v = f32::from_le_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]]);
            (Operand::R4(v), 4)
        }
        Double => {
            need(code, p, 8)?;
            let v = f64::from_le_bytes([
                code[p], code[p + 1], code[p + 2], code[p + 3],
                code[p + 4], code[p + 5], code[p + 6], code[p + 7],
            ]);
            (Operand::R8(v), 8)
        }
        BrTarget => {
            need(code, p, 4)?;
            let v = i32::from_le_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]]);
            (Operand::BrTarget(v), 4)
        }
        ShortBrTarget => {
            let v = (*code.get(p).ok_or_else(|| Error::InvalidCil("missing branch operand".into()))?) as i8;
            (Operand::ShortBrTarget(v), 1)
        }
        Switch => {
            need(code, p, 4)?;
            let n = i32::from_le_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]]) as usize;
            need(code, p + 4, n * 4)?;
            let mut targets = Vec::with_capacity(n);
            for i in 0..n {
                let o = i32::from_le_bytes([
                    code[p + 4 + i * 4],
                    code[p + 4 + i * 4 + 1],
                    code[p + 4 + i * 4 + 2],
                    code[p + 4 + i * 4 + 3],
                ]);
                targets.push(o);
            }
            (Operand::Switch(targets), 4 + n * 4)
        }
        StringTok | FieldTok | MethodTok | TypeTok | Tok | SigTok => {
            need(code, p, 4)?;
            let v = u32::from_le_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]]);
            (Operand::Token(v), 4)
        }
        ShortVar => {
            let v = *code.get(p).ok_or_else(|| Error::InvalidCil("missing var operand".into()))?;
            (Operand::ShortVar(v), 1)
        }
        Var => {
            need(code, p, 2)?;
            let v = u16::from_le_bytes([code[p], code[p + 1]]);
            (Operand::Var(v), 2)
        }
    })
}

fn need(code: &[u8], p: usize, n: usize) -> Result<()> {
    if p + n > code.len() {
        return Err(Error::InvalidCil(format!("operand needs {n} bytes at {p}")));
    }
    Ok(())
}
