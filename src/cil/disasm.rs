use crate::cil::decoder::{decode, Instruction, Operand};
use crate::error::Result;
use crate::metadata::reader::Reader;
use crate::metadata::tables::tbl;

/// Disassemble a method body's bytecode into IL text.
pub fn disassemble(reader: &Reader<'_>, code: &[u8]) -> Result<String> {
    let instrs = decode(code)?;
    let mut s = String::new();
    for ins in &instrs {
        s.push_str(&format_instruction(reader, ins, &instrs));
        s.push('\n');
    }
    Ok(s)
}

fn format_instruction(reader: &Reader<'_>, ins: &Instruction, all: &[Instruction]) -> String {
    let label = format!("IL_{:04X}", ins.offset);
    let operand = format_operand(reader, ins, all);
    if operand.is_empty() {
        format!("  {label}: {name}", name = ins.name)
    } else {
        format!("  {label}: {name} {operand}", name = ins.name)
    }
}

fn format_operand(reader: &Reader<'_>, ins: &Instruction, all: &[Instruction]) -> String {
    match &ins.operand {
        Operand::None => String::new(),
        Operand::I8(v) => format!("0x{:X}", *v as u8),
        Operand::I16(v) => v.to_string(),
        Operand::I32(v) => v.to_string(),
        Operand::I64(v) => format!("0x{:X}", v),
        Operand::R4(v) => format!("{v}"),
        Operand::R8(v) => format!("{v}"),
        Operand::BrTarget(o) => label_for(all, target_off(ins.offset, ins.size, *o as i32)),
        Operand::ShortBrTarget(o) => label_for(all, target_off(ins.offset, ins.size, *o as i32)),
        Operand::Switch(targets) => {
            let base = ins.offset + ins.size;
            let labels: Vec<String> = targets
                .iter()
                .map(|t| label_for(all, (base as i64 + *t as i64).max(0) as usize))
                .collect();
            format!("({})", labels.join(", "))
        }
        Operand::Token(tok) => format_token(reader, *tok),
        Operand::ShortVar(i) => format!("V_{i}"),
        Operand::Var(i) => format!("V_{i}"),
    }
}

fn label_for(all: &[Instruction], target: usize) -> String {
    // Verify the target is an instruction boundary; if not, still emit a label.
    let _ = all;
    format!("IL_{:04X}", target)
}

fn target_off(instr_offset: usize, instr_size: usize, rel: i32) -> usize {
    let base = instr_offset as i64 + instr_size as i64;
    (base + rel as i64).max(0) as usize
}

pub fn format_token(reader: &Reader<'_>, tok: u32) -> String {
    let table = (tok >> 24) as u8;
    let row = (tok & 0x00FF_FFFF) as usize;
    // User-string token (ldstr): table 0x70, index into the #US heap.
    if table == 0x70 {
        let s = reader.root.get_user_string(row as u32).unwrap_or_default();
        return quote_string(&s);
    }
    match table {
        tbl::TYPEREF => {
            if let Some(r) = reader.tables.get(tbl::TYPEREF).get(row - 1) {
                let ns = reader.type_ref_namespace(r);
                let name = reader.type_ref_name(r);
                return format_token_str(ns, name);
            }
        }
        tbl::TYPEDEF => {
            if let Some(r) = reader.tables.get(tbl::TYPEDEF).get(row - 1) {
                let ns = reader.type_def_namespace(r);
                let name = reader.type_def_name(r);
                return format_token_str(ns, name);
            }
        }
        tbl::TYPESPEC => {
            if let Some(r) = reader.tables.get(tbl::TYPESPEC).get(row - 1) {
                if let Ok(t) = crate::metadata::signatures::parse_type(reader.blob(r.col(0))) {
                    return reader.type_name(&t);
                }
            }
        }
        tbl::METHODDEF => {
            if let Some(r) = reader.tables.get(tbl::METHODDEF).get(row - 1) {
                return format!("{}::{}", "<Type>", reader.method_name(r));
            }
        }
        tbl::MEMBERREF => {
            if let Some(r) = reader.tables.get(tbl::MEMBERREF).get(row - 1) {
                let parent = reader.member_ref_parent(r);
                return format!("{}::{}", reader.type_def_or_ref_name(parent), reader.member_ref_name(r));
            }
        }
        tbl::FIELD => {
            if let Some(r) = reader.tables.get(tbl::FIELD).get(row - 1) {
                return reader.field_name(r);
            }
        }
        tbl::STANDALONESIG => return format!("STANDALONESIG({row})"),
        _ => {}
    }
    format!("[{table:#X}:{row}]")
}

fn format_token_str(ns: String, name: String) -> String {
    if ns.is_empty() {
        name
    } else {
        format!("{ns}.{name}")
    }
}

/// Quote a user string for IL disassembly output (ildasm-style).
fn quote_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
