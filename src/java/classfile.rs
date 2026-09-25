//! Java class file parser (JVMS chapter 4).
//!
//! Parses the binary class format (`0xCAFEBABE` magic) into a `ClassFile`:
//! constant pool, access flags, fields, methods and attributes. Only the
//! attributes needed for decompilation are decoded structurally (`Code`,
//! `ConstantValue`, `Exceptions`, `InnerClasses`, `SourceFile`,
//! `LocalVariableTable`); everything else is retained as raw bytes.

use crate::error::{Error, Result};

/// A parsed Java class file.
#[derive(Debug)]
pub struct ClassFile {
    pub minor: u16,
    pub major: u16,
    /// Constant pool entries; index 0 is unused (JVMS 4.4). `Long`/`Double`
    /// occupy two slots — the second slot holds `Cp::Phantom`.
    pub constant_pool: Vec<Cp>,
    pub access_flags: u16,
    /// Constant pool index of the `Class` entry for this type.
    pub this_class: u16,
    /// Constant pool index of the `Class` entry for the superclass (0 for
    /// `java.lang.Object`-rooted interfaces / `Object` itself).
    pub super_class: u16,
    /// Constant pool indices of implemented/extended interfaces.
    pub interfaces: Vec<u16>,
    pub fields: Vec<Member>,
    pub methods: Vec<Member>,
    pub attributes: Vec<Attribute>,
    /// BootstrapMethods attribute (JVMS 4.7.23); needed for `invokedynamic`.
    pub bootstrap_methods: Vec<BootstrapMethod>,
}

/// One entry of the BootstrapMethods attribute.
#[derive(Debug, Clone)]
pub struct BootstrapMethod {
    /// Constant pool index of a MethodHandle.
    pub method_ref: u16,
    /// Constant pool indices of the bootstrap arguments.
    pub arguments: Vec<u16>,
}

/// A field or method entry (JVMS 4.5 / 4.6).
#[derive(Debug)]
pub struct Member {
    pub access_flags: u16,
    pub name: String,
    pub descriptor: String,
    pub attributes: Vec<Attribute>,
}

impl Member {
    pub fn code(&self) -> Option<&Code> {
        self.attributes.iter().find_map(|a| match a {
            Attribute::Code(c) => Some(c),
            _ => None,
        })
    }

    /// Resolved `throws` clause (Exceptions attribute).
    pub fn throws(&self) -> Vec<String> {
        self.attributes
            .iter()
            .find_map(|a| match a {
                Attribute::Exceptions(names) => Some(names.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// ConstantValue attribute (static final fields).
    pub fn constant_value(&self) -> Option<Const> {
        self.attributes.iter().find_map(|a| match a {
            Attribute::ConstantValue(c) => Some(c.clone()),
            _ => None,
        })
    }
}

/// A structured attribute we care about; unknown attributes are skipped.
#[derive(Debug)]
pub enum Attribute {
    Code(Code),
    ConstantValue(Const),
    Exceptions(Vec<String>),
    SourceFile(String),
    /// InnerClasses entries: (inner, outer, inner_name, access_flags).
    InnerClasses(Vec<InnerClass>),
    /// LocalVariableTable entries for one method.
    LocalVarTable(Vec<LocalVar>),
    /// Generic signature (JVMS 4.7.9) — raw string.
    Signature(String),
}

#[derive(Debug)]
pub struct InnerClass {
    /// Binary name (e.g. `com/example/Outer$Inner`).
    pub inner_class: String,
    pub outer_class: Option<String>,
    pub inner_name: Option<String>,
    pub access_flags: u16,
}

/// One local variable slot range from LocalVariableTable.
#[derive(Debug, Clone)]
pub struct LocalVar {
    pub start_pc: u16,
    pub length: u16,
    pub name: String,
    pub descriptor: String,
    pub index: u16,
}

/// Decoded `Code` attribute (JVMS 4.7.3).
#[derive(Debug)]
pub struct Code {
    pub max_stack: u16,
    pub max_locals: u16,
    pub code: Vec<u8>,
    pub exception_table: Vec<ExceptionEntry>,
    pub local_vars: Vec<LocalVar>,
}

/// One exception table entry (JVMS 4.7.3). `catch_type == 0` means `finally`.
#[derive(Debug, Clone)]
pub struct ExceptionEntry {
    pub start_pc: u16,
    pub end_pc: u16,
    pub handler_pc: u16,
    /// Constant pool index of a `Class` entry, or 0 for catch-all/finally.
    pub catch_type: u16,
}

/// A loadable constant (JVMS 4.4) — what `ldc`/`ldc_w`/`ldc2_w` push.
#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Str(String),
    /// `ldc Class` (class literal): binary name.
    Class(String),
    /// MethodType / MethodHandle / Dynamic constants — rendered symbolically.
    Other(String),
}

/// Constant pool entries (JVMS 4.4).
#[derive(Debug, Clone)]
pub enum Cp {
    Utf8(String),
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    Class(u16),
    String(u16),
    Fieldref { class: u16, nat: u16 },
    Methodref { class: u16, nat: u16 },
    InterfaceMethodref { class: u16, nat: u16 },
    NameAndType { name: u16, descriptor: u16 },
    MethodHandle { kind: u8, reference: u16 },
    MethodType(u16),
    Dynamic { attr: u16, nat: u16 },
    InvokeDynamic { attr: u16, nat: u16 },
    Module(u16),
    Package(u16),
    /// Second slot of a Long/Double pair.
    Phantom,
}

impl ClassFile {
    /// Parse a full class file image.
    pub fn parse(data: &[u8]) -> Result<ClassFile> {
        let mut p = Reader { data, pos: 0 };
        let magic = p.u32()?;
        if magic != 0xCAFEBABE {
            return Err(Error::InvalidClassFile(format!(
                "bad magic {magic:#010X} (expected 0xCAFEBABE)"
            )));
        }
        let minor = p.u16()?;
        let major = p.u16()?;
        let count = p.u16()? as usize; // constant_pool_count: index 0 unused
        let mut constant_pool = Vec::with_capacity(count);
        constant_pool.push(Cp::Phantom); // slot 0
        let mut i = 1;
        while i < count {
            let tag = p.u8()?;
            let cp = match tag {
                1 => {
                    let len = p.u16()? as usize;
                    let bytes = p.bytes(len)?;
                    Cp::Utf8(decode_modified_utf8(bytes))
                }
                3 => Cp::Integer(p.u32()? as i32),
                4 => Cp::Float(f32::from_bits(p.u32()?)),
                5 => {
                    let hi = p.u32()? as u64;
                    let lo = p.u32()? as u64;
                    constant_pool.push(Cp::Long(((hi << 32) | lo) as i64));
                    constant_pool.push(Cp::Phantom);
                    i += 2;
                    continue;
                }
                6 => {
                    let hi = p.u32()? as u64;
                    let lo = p.u32()? as u64;
                    constant_pool.push(Cp::Double(f64::from_bits((hi << 32) | lo)));
                    constant_pool.push(Cp::Phantom);
                    i += 2;
                    continue;
                }
                7 => Cp::Class(p.u16()?),
                8 => Cp::String(p.u16()?),
                9 => Cp::Fieldref { class: p.u16()?, nat: p.u16()? },
                10 => Cp::Methodref { class: p.u16()?, nat: p.u16()? },
                11 => Cp::InterfaceMethodref { class: p.u16()?, nat: p.u16()? },
                12 => Cp::NameAndType { name: p.u16()?, descriptor: p.u16()? },
                15 => Cp::MethodHandle { kind: p.u8()?, reference: p.u16()? },
                16 => Cp::MethodType(p.u16()?),
                17 => Cp::Dynamic { attr: p.u16()?, nat: p.u16()? },
                18 => Cp::InvokeDynamic { attr: p.u16()?, nat: p.u16()? },
                19 => Cp::Module(p.u16()?),
                20 => Cp::Package(p.u16()?),
                other => {
                    return Err(Error::InvalidClassFile(format!(
                        "unknown constant pool tag {other} at entry {i}"
                    )))
                }
            };
            constant_pool.push(cp);
            i += 1;
        }

        let access_flags = p.u16()?;
        let this_class = p.u16()?;
        let super_class = p.u16()?;
        let iface_count = p.u16()? as usize;
        let interfaces = (0..iface_count).map(|_| p.u16()).collect::<Result<Vec<_>>>()?;

        let fields = read_members(&mut p, &constant_pool)?;
        let methods = read_members(&mut p, &constant_pool)?;
        let raw_attrs = read_raw_attributes(&mut p, &constant_pool)?;
        let attributes = decode_attributes(&raw_attrs, &constant_pool)?;

        // BootstrapMethods is a class-level attribute kept in raw form.
        let mut bootstrap_methods = Vec::new();
        for (name, data) in &raw_attrs {
            if name == "BootstrapMethods" {
                let mut r = Reader { data, pos: 0 };
                let n = r.u16()? as usize;
                for _ in 0..n {
                    let method_ref = r.u16()?;
                    let argc = r.u16()? as usize;
                    let arguments = (0..argc).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
                    bootstrap_methods.push(BootstrapMethod { method_ref, arguments });
                }
            }
        }

        Ok(ClassFile {
            minor,
            major,
            constant_pool,
            access_flags,
            this_class,
            super_class,
            interfaces,
            fields,
            methods,
            attributes,
            bootstrap_methods,
        })
    }

    // ---- constant pool accessors -------------------------------------------

    pub fn utf8(&self, idx: u16) -> &str {
        match self.constant_pool.get(idx as usize) {
            Some(Cp::Utf8(s)) => s,
            _ => "<bad utf8>",
        }
    }

    /// Binary name of a `Class` entry (e.g. `java/lang/String`).
    pub fn class_name(&self, idx: u16) -> String {
        match self.constant_pool.get(idx as usize) {
            Some(Cp::Class(name_idx)) => self.utf8(*name_idx).to_string(),
            _ => "<bad class>".into(),
        }
    }

    pub fn name_and_type(&self, idx: u16) -> (String, String) {
        match self.constant_pool.get(idx as usize) {
            Some(Cp::NameAndType { name, descriptor }) => {
                (self.utf8(*name).to_string(), self.utf8(*descriptor).to_string())
            }
            _ => ("<bad nat>".into(), "".into()),
        }
    }

    /// Resolve a Fieldref/Methodref/InterfaceMethodref to
    /// `(class binary name, member name, descriptor)`.
    pub fn member_ref(&self, idx: u16) -> (String, String, String) {
        let (class, nat) = match self.constant_pool.get(idx as usize) {
            Some(Cp::Fieldref { class, nat })
            | Some(Cp::Methodref { class, nat })
            | Some(Cp::InterfaceMethodref { class, nat }) => (*class, *nat),
            _ => return ("<bad ref>".into(), "".into(), "".into()),
        };
        let (name, desc) = self.name_and_type(nat);
        (self.class_name(class), name, desc)
    }

    /// Resolve a loadable constant for `ldc`-family instructions.
    pub fn constant(&self, idx: u16) -> Const {
        match self.constant_pool.get(idx as usize) {
            Some(Cp::Integer(v)) => Const::Int(*v),
            Some(Cp::Long(v)) => Const::Long(*v),
            Some(Cp::Float(v)) => Const::Float(*v),
            Some(Cp::Double(v)) => Const::Double(*v),
            Some(Cp::String(s)) => Const::Str(self.utf8(*s).to_string()),
            Some(Cp::Class(n)) => Const::Class(self.utf8(*n).to_string()),
            Some(Cp::MethodType(d)) => Const::Other(format!("methodtype {}", self.utf8(*d))),
            Some(Cp::MethodHandle { .. }) => Const::Other("methodhandle".into()),
            Some(Cp::Dynamic { .. }) | Some(Cp::InvokeDynamic { .. }) => {
                Const::Other("dynamic".into())
            }
            _ => Const::Other("<bad ldc>".into()),
        }
    }

    /// Fully-qualified name of this class, dotted (e.g. `com.example.Foo`).
    pub fn this_name(&self) -> String {
        to_dotted(&self.class_name(self.this_class))
    }

    /// Package part of this class's name (`""` for the default package).
    pub fn package(&self) -> String {
        let full = self.this_name();
        match full.rfind('.') {
            Some(i) => full[..i].to_string(),
            None => String::new(),
        }
    }

    /// Simple name of this class (last `.`-separated segment).
    pub fn simple_name(&self) -> String {
        let full = self.this_name();
        full[full.rfind('.').map(|i| i + 1).unwrap_or(0)..].to_string()
    }

    /// Superclass dotted name; `None` for `java.lang.Object` itself.
    pub fn super_name(&self) -> Option<String> {
        if self.super_class == 0 {
            return None;
        }
        let name = to_dotted(&self.class_name(self.super_class));
        if name == "java.lang.Object" {
            None
        } else {
            Some(name)
        }
    }

    pub fn interface_names(&self) -> Vec<String> {
        self.interfaces
            .iter()
            .map(|i| to_dotted(&self.class_name(*i)))
            .collect()
    }

    pub fn source_file(&self) -> Option<&str> {
        self.attributes.iter().find_map(|a| match a {
            Attribute::SourceFile(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// Static initializer (`<clinit>`) method, if it has code.
    pub fn clinit(&self) -> Option<&Member> {
        self.methods
            .iter()
            .find(|m| m.name == "<clinit>" && m.code().is_some())
    }
}

// ---- access flag constants (JVMS tables 4.1-B, 4.5-A, 4.6-A) ----------------

pub mod acc {
    pub const PUBLIC: u16 = 0x0001;
    pub const PRIVATE: u16 = 0x0002;
    pub const PROTECTED: u16 = 0x0004;
    pub const STATIC: u16 = 0x0008;
    pub const FINAL: u16 = 0x0010;
    pub const SYNCHRONIZED_SUPER: u16 = 0x0020;
    pub const VOLATILE_BRIDGE: u16 = 0x0040;
    /// Same bit as VOLATILE_BRIDGE — `bridge` on methods (JVMS 4.6).
    pub const BRIDGE: u16 = 0x0040;
    pub const TRANSIENT_VARARGS: u16 = 0x0080;
    pub const NATIVE: u16 = 0x0100;
    pub const INTERFACE: u16 = 0x0200;
    pub const ABSTRACT: u16 = 0x0400;
    pub const STRICT: u16 = 0x0800;
    pub const SYNTHETIC: u16 = 0x1000;
    pub const ANNOTATION: u16 = 0x2000;
    pub const ENUM: u16 = 0x4000;
}

// ---- low-level reader -------------------------------------------------------

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn need(&self, n: usize) -> Result<()> {
        if self.pos + n > self.data.len() {
            return Err(Error::InvalidClassFile(format!(
                "truncated class file: need {n} bytes at offset {}",
                self.pos
            )));
        }
        Ok(())
    }
    fn u8(&mut self) -> Result<u8> {
        self.need(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16> {
        self.need(2)?;
        let v = u16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32> {
        self.need(4)?;
        let v = u32::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        self.need(n)?;
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

fn read_members(p: &mut Reader, cp: &[Cp]) -> Result<Vec<Member>> {
    let count = p.u16()? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let access_flags = p.u16()?;
        let name = resolve_utf8(cp, p.u16()?)?;
        let descriptor = resolve_utf8(cp, p.u16()?)?;
        let attributes = read_attributes(p, cp)?;
        out.push(Member { access_flags, name, descriptor, attributes });
    }
    Ok(out)
}

/// Read raw attributes as (name, data) pairs (JVMS 4.7).
fn read_raw_attributes(p: &mut Reader, cp: &[Cp]) -> Result<Vec<(String, Vec<u8>)>> {
    let count = p.u16()? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let name = resolve_utf8(cp, p.u16()?)?;
        let len = p.u32()? as usize;
        let data = p.bytes(len)?.to_vec();
        out.push((name, data));
    }
    Ok(out)
}

fn read_attributes(p: &mut Reader, cp: &[Cp]) -> Result<Vec<Attribute>> {
    let raw = read_raw_attributes(p, cp)?;
    decode_attributes(&raw, cp)
}

/// Decode raw (name, data) attribute pairs into the structured subset we
/// understand; unknown attributes are skipped.
fn decode_attributes(raw: &[(String, Vec<u8>)], cp: &[Cp]) -> Result<Vec<Attribute>> {
    let mut out = Vec::new();
    for (name, data) in raw {
        let data = data.clone();
        let attr = match name.as_str() {
            "Code" => parse_code(&data, cp).map(Attribute::Code),
            "ConstantValue" => {
                let mut r = Reader { data: &data, pos: 0 };
                let idx = r.u16()?;
                parse_const(cp, idx).map(Attribute::ConstantValue)
            }
            "Exceptions" => {
                let mut r = Reader { data: &data, pos: 0 };
                let n = r.u16()? as usize;
                let mut names = Vec::with_capacity(n);
                for _ in 0..n {
                    let ci = r.u16()?;
                    names.push(class_binary_name(cp, ci));
                }
                Ok(Attribute::Exceptions(names))
            }
            "SourceFile" => {
                let mut r = Reader { data: &data, pos: 0 };
                resolve_utf8(cp, r.u16()?).map(Attribute::SourceFile)
            }
            "InnerClasses" => parse_inner_classes(&data, cp),
            "LocalVariableTable" => parse_lvt(&data, cp),
            "Signature" => {
                let mut r = Reader { data: &data, pos: 0 };
                resolve_utf8(cp, r.u16()?).map(Attribute::Signature)
            }
            _ => continue, // unknown / not needed
        };
        if let Ok(a) = attr {
            out.push(a);
        }
    }
    Ok(out)
}

fn parse_code(data: &[u8], cp: &[Cp]) -> Result<Code> {
    let mut r = Reader { data, pos: 0 };
    let max_stack = r.u16()?;
    let max_locals = r.u16()?;
    let code_len = r.u32()? as usize;
    let code = r.bytes(code_len)?.to_vec();
    let exc_count = r.u16()? as usize;
    let mut exception_table = Vec::with_capacity(exc_count);
    for _ in 0..exc_count {
        exception_table.push(ExceptionEntry {
            start_pc: r.u16()?,
            end_pc: r.u16()?,
            handler_pc: r.u16()?,
            catch_type: r.u16()?,
        });
    }
    // Sub-attributes: look for LocalVariableTable only.
    let sub = read_attributes(&mut r, cp)?;
    let local_vars = sub
        .iter()
        .find_map(|a| match a {
            Attribute::LocalVarTable(v) => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_default();
    Ok(Code { max_stack, max_locals, code, exception_table, local_vars })
}

fn parse_inner_classes(data: &[u8], cp: &[Cp]) -> Result<Attribute> {
    let mut r = Reader { data, pos: 0 };
    let n = r.u16()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let inner = class_binary_name(cp, r.u16()?);
        let outer_idx = r.u16()?;
        let name_idx = r.u16()?;
        let flags = r.u16()?;
        let outer_class = if outer_idx == 0 {
            None
        } else {
            Some(class_binary_name(cp, outer_idx))
        };
        let inner_name = if name_idx == 0 {
            None
        } else {
            Some(resolve_utf8(cp, name_idx)?)
        };
        out.push(InnerClass { inner_class: inner, outer_class, inner_name, access_flags: flags });
    }
    Ok(Attribute::InnerClasses(out))
}

fn parse_lvt(data: &[u8], cp: &[Cp]) -> Result<Attribute> {
    let mut r = Reader { data, pos: 0 };
    let n = r.u16()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(LocalVar {
            start_pc: r.u16()?,
            length: r.u16()?,
            name: resolve_utf8(cp, r.u16()?)?,
            descriptor: resolve_utf8(cp, r.u16()?)?,
            index: r.u16()?,
        });
    }
    Ok(Attribute::LocalVarTable(out))
}

fn resolve_utf8(cp: &[Cp], idx: u16) -> Result<String> {
    match cp.get(idx as usize) {
        Some(Cp::Utf8(s)) => Ok(s.clone()),
        _ => Err(Error::InvalidClassFile(format!(
            "constant pool index {idx} is not a Utf8 entry"
        ))),
    }
}

/// Binary (slash) name of the `Class` entry at `idx`, or `"<bad class>"`.
fn class_binary_name(cp: &[Cp], idx: u16) -> String {
    match cp.get(idx as usize) {
        Some(Cp::Class(name_idx)) => match cp.get(*name_idx as usize) {
            Some(Cp::Utf8(s)) => s.clone(),
            _ => "<bad class>".into(),
        },
        _ => "<bad class>".into(),
    }
}

fn parse_const(cp: &[Cp], idx: u16) -> Result<Const> {
    match cp.get(idx as usize) {
        Some(Cp::Integer(v)) => Ok(Const::Int(*v)),
        Some(Cp::Long(v)) => Ok(Const::Long(*v)),
        Some(Cp::Float(v)) => Ok(Const::Float(*v)),
        Some(Cp::Double(v)) => Ok(Const::Double(*v)),
        Some(Cp::String(s)) => match cp.get(*s as usize) {
            Some(Cp::Utf8(v)) => Ok(Const::Str(v.clone())),
            _ => Err(Error::InvalidClassFile("bad String constant".into())),
        },
        _ => Err(Error::InvalidClassFile(format!(
            "constant pool index {idx} has no loadable constant"
        ))),
    }
}

/// Convert a binary name (`com/example/Foo$Bar`) to dotted display form
/// (`com.example.Foo.Bar`).
pub fn to_dotted(binary: &str) -> String {
    binary.replace(['/', '$'], ".")
}

/// Decode JVM "modified UTF-8" (JVMS 4.4.7): U+0000 is encoded as `C0 80`
/// and supplementary characters as surrogate pairs encoded in 3-byte units.
fn decode_modified_utf8(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            out.push(if b == 0 { '\u{FFFD}' } else { b as char });
            i += 1;
        } else if b >> 5 == 0b110 && bytes.len() > i + 1 {
            let cp = (((b & 0x1F) as u32) << 6) | (bytes[i + 1] & 0x3F) as u32;
            out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            i += 2;
        } else if b >> 4 == 0b1110 && bytes.len() > i + 2 {
            let cp = (((b & 0x0F) as u32) << 12)
                | (((bytes[i + 1] & 0x3F) as u32) << 6)
                | (bytes[i + 2] & 0x3F) as u32;
            out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            i += 3;
        } else {
            out.push('\u{FFFD}');
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_utf8_ascii() {
        assert_eq!(decode_modified_utf8(b"hello"), "hello");
    }

    #[test]
    fn modified_utf8_nul_is_c0_80() {
        // JVMS 4.4.7: U+0000 is encoded as C0 80 (not the single byte 00).
        assert_eq!(decode_modified_utf8(&[0xC0, 0x80]), "\u{0}");
    }

    #[test]
    fn modified_utf8_two_byte() {
        // é = U+00E9 → C3 A9
        assert_eq!(decode_modified_utf8(&[0xC3, 0xA9]), "é");
    }

    #[test]
    fn modified_utf8_truncated_is_replacement() {
        assert_eq!(decode_modified_utf8(&[0xE4]), "\u{FFFD}");
    }

    #[test]
    fn to_dotted_converts_slashes_and_dollars() {
        assert_eq!(to_dotted("com/example/Outer$Inner"), "com.example.Outer.Inner");
        assert_eq!(to_dotted("Foo"), "Foo");
    }

    #[test]
    fn rejects_bad_magic() {
        let err = ClassFile::parse(&[0x00, 0x01, 0x02, 0x03]).unwrap_err();
        assert!(matches!(err, Error::InvalidClassFile(_)));
    }

    #[test]
    fn rejects_truncated() {
        let err = ClassFile::parse(&[0xCA, 0xFE, 0xBA, 0xBE, 0x00]).unwrap_err();
        assert!(matches!(err, Error::InvalidClassFile(_)));
    }
}
