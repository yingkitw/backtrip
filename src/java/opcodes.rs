//! JVM instruction set (JVMS chapter 6).
//!
//! One entry per opcode byte (0x00–0xC9). Operand kinds mirror JVMS;
//! branch offsets are relative to the *address of the opcode* (unlike CIL,
//! where they are relative to the following instruction).

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    None,
    /// `bipush` — signed byte.
    SByte,
    /// `sipush` — signed short.
    SShort,
    /// `ldc` — 1-byte constant pool index.
    Cp1,
    /// 2-byte constant pool index (`ldc_w`, field/method refs, `new`, ...).
    Cp2,
    /// `invokeinterface` — Cp2 + count byte + zero byte.
    Cp2Extra1,
    /// `invokedynamic` — Cp2 + 2 zero bytes.
    Cp2Extra2,
    /// 1-byte local variable index (`iload`, `istore`, `ret`, ...).
    Local,
    /// `iinc` — local index + signed delta.
    Iinc,
    /// `newarray` — primitive array type code.
    AType,
    /// `multianewarray` — Cp2 + dimension count.
    Dims,
    /// 16-bit branch offset, relative to the opcode address.
    Branch,
    /// 32-bit branch offset (`goto_w`, `jsr_w`).
    BranchW,
    TableSwitch,
    LookupSwitch,
}

#[derive(Debug, Clone, Copy)]
pub struct OpInfo {
    pub name: &'static str,
    pub kind: Kind,
}

const N: OpInfo = OpInfo { name: "unknown", kind: Kind::None };

pub fn lookup(op: u8) -> OpInfo {
    TABLE[op as usize]
}

macro_rules! ops {
    ($t:ident, $($val:literal => $name:literal, $kind:ident;)*) => {
        $($t[$val] = OpInfo { name: $name, kind: Kind::$kind };)*
    };
}

static TABLE: [OpInfo; 0xCA] = {
    let mut t = [N; 0xCA];
    ops! {
        t,
        0x00 => "nop", None;
        0x01 => "aconst_null", None;
        0x02 => "iconst_m1", None;
        0x03 => "iconst_0", None;
        0x04 => "iconst_1", None;
        0x05 => "iconst_2", None;
        0x06 => "iconst_3", None;
        0x07 => "iconst_4", None;
        0x08 => "iconst_5", None;
        0x09 => "lconst_0", None;
        0x0A => "lconst_1", None;
        0x0B => "fconst_0", None;
        0x0C => "fconst_1", None;
        0x0D => "fconst_2", None;
        0x0E => "dconst_0", None;
        0x0F => "dconst_1", None;
        0x10 => "bipush", SByte;
        0x11 => "sipush", SShort;
        0x12 => "ldc", Cp1;
        0x13 => "ldc_w", Cp2;
        0x14 => "ldc2_w", Cp2;
        0x15 => "iload", Local;
        0x16 => "lload", Local;
        0x17 => "fload", Local;
        0x18 => "dload", Local;
        0x19 => "aload", Local;
        0x1A => "iload_0", None;
        0x1B => "iload_1", None;
        0x1C => "iload_2", None;
        0x1D => "iload_3", None;
        0x1E => "lload_0", None;
        0x1F => "lload_1", None;
        0x20 => "lload_2", None;
        0x21 => "lload_3", None;
        0x22 => "fload_0", None;
        0x23 => "fload_1", None;
        0x24 => "fload_2", None;
        0x25 => "fload_3", None;
        0x26 => "dload_0", None;
        0x27 => "dload_1", None;
        0x28 => "dload_2", None;
        0x29 => "dload_3", None;
        0x2A => "aload_0", None;
        0x2B => "aload_1", None;
        0x2C => "aload_2", None;
        0x2D => "aload_3", None;
        0x2E => "iaload", None;
        0x2F => "laload", None;
        0x30 => "faload", None;
        0x31 => "daload", None;
        0x32 => "aaload", None;
        0x33 => "baload", None;
        0x34 => "caload", None;
        0x35 => "saload", None;
        0x36 => "istore", Local;
        0x37 => "lstore", Local;
        0x38 => "fstore", Local;
        0x39 => "dstore", Local;
        0x3A => "astore", Local;
        0x3B => "istore_0", None;
        0x3C => "istore_1", None;
        0x3D => "istore_2", None;
        0x3E => "istore_3", None;
        0x3F => "lstore_0", None;
        0x40 => "lstore_1", None;
        0x41 => "lstore_2", None;
        0x42 => "lstore_3", None;
        0x43 => "fstore_0", None;
        0x44 => "fstore_1", None;
        0x45 => "fstore_2", None;
        0x46 => "fstore_3", None;
        0x47 => "dstore_0", None;
        0x48 => "dstore_1", None;
        0x49 => "dstore_2", None;
        0x4A => "dstore_3", None;
        0x4B => "astore_0", None;
        0x4C => "astore_1", None;
        0x4D => "astore_2", None;
        0x4E => "astore_3", None;
        0x4F => "iastore", None;
        0x50 => "lastore", None;
        0x51 => "fastore", None;
        0x52 => "dastore", None;
        0x53 => "aastore", None;
        0x54 => "bastore", None;
        0x55 => "castore", None;
        0x56 => "sastore", None;
        0x57 => "pop", None;
        0x58 => "pop2", None;
        0x59 => "dup", None;
        0x5A => "dup_x1", None;
        0x5B => "dup_x2", None;
        0x5C => "dup2", None;
        0x5D => "dup2_x1", None;
        0x5E => "dup2_x2", None;
        0x5F => "swap", None;
        0x60 => "iadd", None;
        0x61 => "ladd", None;
        0x62 => "fadd", None;
        0x63 => "dadd", None;
        0x64 => "isub", None;
        0x65 => "lsub", None;
        0x66 => "fsub", None;
        0x67 => "dsub", None;
        0x68 => "imul", None;
        0x69 => "lmul", None;
        0x6A => "fmul", None;
        0x6B => "dmul", None;
        0x6C => "idiv", None;
        0x6D => "ldiv", None;
        0x6E => "fdiv", None;
        0x6F => "ddiv", None;
        0x70 => "irem", None;
        0x71 => "lrem", None;
        0x72 => "frem", None;
        0x73 => "drem", None;
        0x74 => "ineg", None;
        0x75 => "lneg", None;
        0x76 => "fneg", None;
        0x77 => "dneg", None;
        0x78 => "ishl", None;
        0x79 => "lshl", None;
        0x7A => "ishr", None;
        0x7B => "lshr", None;
        0x7C => "iushr", None;
        0x7D => "lushr", None;
        0x7E => "iand", None;
        0x7F => "land", None;
        0x80 => "ior", None;
        0x81 => "lor", None;
        0x82 => "ixor", None;
        0x83 => "lxor", None;
        0x84 => "iinc", Iinc;
        0x85 => "i2l", None;
        0x86 => "i2f", None;
        0x87 => "i2d", None;
        0x88 => "l2i", None;
        0x89 => "l2f", None;
        0x8A => "l2d", None;
        0x8B => "f2i", None;
        0x8C => "f2l", None;
        0x8D => "f2d", None;
        0x8E => "d2i", None;
        0x8F => "d2l", None;
        0x90 => "d2f", None;
        0x91 => "i2b", None;
        0x92 => "i2c", None;
        0x93 => "i2s", None;
        0x94 => "lcmp", None;
        0x95 => "fcmpl", None;
        0x96 => "fcmpg", None;
        0x97 => "dcmpl", None;
        0x98 => "dcmpg", None;
        0x99 => "ifeq", Branch;
        0x9A => "ifne", Branch;
        0x9B => "iflt", Branch;
        0x9C => "ifge", Branch;
        0x9D => "ifgt", Branch;
        0x9E => "ifle", Branch;
        0x9F => "if_icmpeq", Branch;
        0xA0 => "if_icmpne", Branch;
        0xA1 => "if_icmplt", Branch;
        0xA2 => "if_icmpge", Branch;
        0xA3 => "if_icmpgt", Branch;
        0xA4 => "if_icmple", Branch;
        0xA5 => "if_acmpeq", Branch;
        0xA6 => "if_acmpne", Branch;
        0xA7 => "goto", Branch;
        0xA8 => "jsr", Branch;
        0xA9 => "ret", Local;
        0xAA => "tableswitch", TableSwitch;
        0xAB => "lookupswitch", LookupSwitch;
        0xAC => "ireturn", None;
        0xAD => "lreturn", None;
        0xAE => "freturn", None;
        0xAF => "dreturn", None;
        0xB0 => "areturn", None;
        0xB1 => "return", None;
        0xB2 => "getstatic", Cp2;
        0xB3 => "putstatic", Cp2;
        0xB4 => "getfield", Cp2;
        0xB5 => "putfield", Cp2;
        0xB6 => "invokevirtual", Cp2;
        0xB7 => "invokespecial", Cp2;
        0xB8 => "invokestatic", Cp2;
        0xB9 => "invokeinterface", Cp2Extra1;
        0xBA => "invokedynamic", Cp2Extra2;
        0xBB => "new", Cp2;
        0xBC => "newarray", AType;
        0xBD => "anewarray", Cp2;
        0xBE => "arraylength", None;
        0xBF => "athrow", None;
        0xC0 => "checkcast", Cp2;
        0xC1 => "instanceof", Cp2;
        0xC2 => "monitorenter", None;
        0xC3 => "monitorexit", None;
        0xC4 => "wide", None;
        0xC5 => "multianewarray", Dims;
        0xC6 => "ifnull", Branch;
        0xC7 => "ifnonnull", Branch;
        0xC8 => "goto_w", BranchW;
        0xC9 => "jsr_w", BranchW;
    }
    t
};
/// Primitive type for a `newarray` type code (JVMS 6.5).
pub fn newarray_type(atype: u8) -> &'static str {
    match atype {
        4 => "boolean",
        5 => "char",
        6 => "float",
        7 => "double",
        8 => "byte",
        9 => "short",
        10 => "int",
        11 => "long",
        _ => "int",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_complete() {
        // Every opcode byte 0x00..=0xC9 must resolve to a named instruction
        // (no "unknown" gaps in the JVMS range).
        for b in 0x00..=0xC9 {
            let info = lookup(b);
            assert_ne!(info.name, "unknown", "opcode {b:#04X} is unnamed");
        }
    }

    #[test]
    fn operand_kinds() {
        assert_eq!(lookup(0x00).name, "nop");
        assert_eq!(lookup(0x10).kind, Kind::SByte); // bipush
        assert_eq!(lookup(0x12).kind, Kind::Cp1); // ldc
        assert_eq!(lookup(0xB6).kind, Kind::Cp2); // invokevirtual
        assert_eq!(lookup(0xB9).kind, Kind::Cp2Extra1); // invokeinterface
        assert_eq!(lookup(0x84).kind, Kind::Iinc); // iinc
        assert_eq!(lookup(0xAA).kind, Kind::TableSwitch);
        assert_eq!(lookup(0xAB).kind, Kind::LookupSwitch);
        assert_eq!(lookup(0xC4).name, "wide");
    }

    #[test]
    fn newarray_codes() {
        assert_eq!(newarray_type(10), "int");
        assert_eq!(newarray_type(4), "boolean");
    }
}
