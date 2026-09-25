//! javap-style JVM bytecode disassembly.

use crate::error::Result;
use crate::java::classfile::{ClassFile, Const, Member};
use crate::java::decoder::{decode, Operand};
use crate::java::opcodes::newarray_type;

/// Disassemble one method body to javap-style text (no header).
pub fn disassemble_method(cf: &ClassFile, m: &Member) -> Result<String> {
    let mut out = String::new();
    let (params, ret) = crate::java::descriptor::parse_method_descriptor(&m.descriptor)?;
    out.push_str(&format!(
        "  {} {}({}) -> {}\n",
        member_flags(m.access_flags),
        m.name,
        params.join(", "),
        ret
    ));
    if let Some(throws) = m.attributes.iter().find_map(|a| match a {
        crate::java::classfile::Attribute::Exceptions(t) => Some(t.clone()),
        _ => None,
    })
        && !throws.is_empty() {
            out.push_str(&format!("    throws {}\n", throws.join(", ")));
        }
    let Some(code) = m.code() else {
        out.push_str("    // no body (abstract / native)\n");
        return Ok(out);
    };
    out.push_str(&format!(
        "    Code: stack={}, locals={}, args={}\n",
        code.max_stack,
        code.max_locals,
        param_slots(&m.descriptor, m.access_flags)
    ));
    let instructions = decode(&code.code)?;
    for i in &instructions {
        out.push_str(&format!("    {:4}: {}\n", i.offset, format_instruction(cf, i)));
    }
    if !code.exception_table.is_empty() {
        out.push_str("    Exception table:\n");
        for e in &code.exception_table {
            let ty = if e.catch_type == 0 {
                "any".to_string()
            } else {
                crate::java::classfile::to_dotted(&cf.class_name(e.catch_type))
            };
            out.push_str(&format!(
                "       [{:#x}, {:#x}) -> {:#x} catch {}\n",
                e.start_pc, e.end_pc, e.handler_pc, ty
            ));
        }
    }
    Ok(out)
}

/// Disassemble a whole class file (all methods + fields).
pub fn disassemble_class(cf: &ClassFile) -> Result<String> {
    let mut out = String::new();
    out.push_str(&format!(
        "// class version {}.{}, source: {}\n",
        cf.major,
        cf.minor,
        cf.source_file().unwrap_or("<unknown>")
    ));
    out.push_str(&format!("class {}\n{{\n", cf.this_name()));
    for f in &cf.fields {
        let ty = crate::java::descriptor::parse_field_descriptor(&f.descriptor)?;
        out.push_str(&format!("  {} {} {};\n", member_flags(f.access_flags), ty, f.name));
    }
    if !cf.fields.is_empty() {
        out.push('\n');
    }
    for m in &cf.methods {
        out.push_str(&disassemble_method(cf, m)?);
        out.push('\n');
    }
    out.push_str("}\n");
    Ok(out)
}

fn format_instruction(cf: &ClassFile, i: &crate::java::decoder::Instruction) -> String {
    match &i.operand {
        Operand::None => i.name.to_string(),
        Operand::I8(v) => format!("{} {v}", i.name),
        Operand::I16(v) => format!("{} {v}", i.name),
        Operand::I32(v) => format!("{} {v}", i.name),
        Operand::Cp(idx) => format_cp_operand(cf, i.name, *idx),
        Operand::Local(n) => format!("{} {n}", i.name),
        Operand::Iinc(n, d) => format!("{} {n}, {d:+}", i.name),
        Operand::WideIinc(n, d) => format!("{} {n}, {d:+}", i.name),
        Operand::AType(t) => format!("{} {}", i.name, newarray_type(*t)),
        Operand::Dims(idx, n) => format!("{} class #{} dims {n}", i.name, idx),
        Operand::Target(t) => format!("{} {t}", i.name),
        Operand::TableSwitch { low, high, targets, .. } => {
            let cases: Vec<String> =
                targets.iter().enumerate().map(|(k, t)| format!("{}: {t}", low + k as i32)).collect();
            format!("{} low={low} high={high} [{}]", i.name, cases.join(", "))
        }
        Operand::LookupSwitch { pairs, .. } => {
            let cases: Vec<String> =
                pairs.iter().map(|(v, t)| format!("{v}: {t}")).collect();
            format!("{} [{}]", i.name, cases.join(", "))
        }
    }
}

fn format_cp_operand(cf: &ClassFile, name: &str, idx: u16) -> String {
    match name {
        "ldc" | "ldc_w" | "ldc2_w" => match cf.constant(idx) {
            Const::Str(s) => format!("{} \"{}\"", name, s.escape_debug()),
            Const::Int(v) => format!("{name} {v}"),
            Const::Long(v) => format!("{name} {v}L"),
            Const::Float(v) => format!("{name} {v}f"),
            Const::Double(v) => format!("{name} {v}"),
            Const::Class(c) => format!("{name} class {}", to_dotted(&c)),
            Const::Other(o) => format!("{name} {o}"),
        },
        "getstatic" | "putstatic" | "getfield" | "putfield" => {
            let (class, field, desc) = cf.member_ref(idx);
            let ty = crate::java::descriptor::parse_field_descriptor(&desc).unwrap_or(desc);
            format!("{name} {}.{} ({})", to_dotted(&class), field, ty)
        }
        "invokevirtual" | "invokespecial" | "invokestatic" | "invokeinterface" => {
            let (class, method, desc) = cf.member_ref(idx);
            format!("{name} {}.{}{}", to_dotted(&class), method, desc)
        }
        "new" | "anewarray" | "checkcast" | "instanceof" => {
            format!("{name} {}", to_dotted(&cf.class_name(idx)))
        }
        _ => format!("{name} #{idx}"),
    }
}

/// Render member access flags (JVMS 4.5/4.6) as Java keywords.
fn member_flags(flags: u16) -> String {
    use crate::java::classfile::acc;
    let mut out = Vec::new();
    if flags & acc::PUBLIC != 0 {
        out.push("public");
    } else if flags & acc::PRIVATE != 0 {
        out.push("private");
    } else if flags & acc::PROTECTED != 0 {
        out.push("protected");
    }
    if flags & acc::STATIC != 0 {
        out.push("static");
    }
    if flags & acc::FINAL != 0 {
        out.push("final");
    }
    if flags & acc::SYNCHRONIZED_SUPER != 0 {
        out.push("synchronized");
    }
    if flags & acc::VOLATILE_BRIDGE != 0 {
        out.push("volatile");
    }
    if flags & acc::TRANSIENT_VARARGS != 0 {
        out.push("transient");
    }
    if flags & acc::NATIVE != 0 {
        out.push("native");
    }
    if flags & acc::ABSTRACT != 0 {
        out.push("abstract");
    }
    out.join(" ")
}

fn param_slots(desc: &str, flags: u16) -> usize {
    use crate::java::classfile::acc;
    let mut slots = if flags & acc::STATIC != 0 { 0 } else { 1 };
    if let Ok((params, _)) = crate::java::descriptor::parse_method_descriptor(desc) {
        for p in params {
            slots += if p == "long" || p == "double" { 2 } else { 1 };
        }
    }
    slots
}

// Local alias so the match arms above stay readable.
use crate::java::classfile::to_dotted;
