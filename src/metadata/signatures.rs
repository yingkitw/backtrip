use crate::error::{Error, Result};
use crate::metadata::streams::decode_compressed_uint;
use crate::metadata::tables::{decode_coded, Coded, CodedIndex};

/// ECMA-335 element types (II.23.1.16).
pub mod et {
    pub const END: u8 = 0x00;
    pub const VOID: u8 = 0x01;
    pub const BOOLEAN: u8 = 0x02;
    pub const CHAR: u8 = 0x03;
    pub const I1: u8 = 0x04;
    pub const U1: u8 = 0x05;
    pub const I2: u8 = 0x06;
    pub const U2: u8 = 0x07;
    pub const I4: u8 = 0x08;
    pub const U4: u8 = 0x09;
    pub const I8: u8 = 0x0a;
    pub const U8: u8 = 0x0b;
    pub const R4: u8 = 0x0c;
    pub const R8: u8 = 0x0d;
    pub const STRING: u8 = 0x0e;
    pub const PTR: u8 = 0x0f;
    pub const BYREF: u8 = 0x10;
    pub const VALUETYPE: u8 = 0x11;
    pub const CLASS: u8 = 0x12;
    pub const VAR: u8 = 0x13;
    pub const ARRAY: u8 = 0x14;
    pub const GENERICINST: u8 = 0x15;
    pub const TYPEDBYREF: u8 = 0x16;
    pub const I: u8 = 0x18;
    pub const U: u8 = 0x19;
    pub const FNPTR: u8 = 0x1b;
    pub const OBJECT: u8 = 0x1c;
    pub const SZARRAY: u8 = 0x1d;
    pub const MVAR: u8 = 0x1e;
    pub const CMOD_REQD: u8 = 0x1f;
    pub const CMOD_OPT: u8 = 0x20;
    pub const SENTINEL: u8 = 0x41;
    pub const PINNED: u8 = 0x45;
}

#[derive(Debug, Clone)]
pub enum Type {
    Void,
    Bool,
    Char,
    I1,
    U1,
    I2,
    U2,
    I4,
    U4,
    I8,
    U8,
    R4,
    R8,
    String,
    I,
    U,
    Object,
    TypedRef,
    Ptr(Box<Type>),
    ByRef(Box<Type>),
    SzArray(Box<Type>),
    Array(Box<Type>, ArrayShape),
    ValueType(CodedIndex),
    Class(CodedIndex),
    Var(u32),
    MVar(u32),
    GenericInst(Box<Type>, Vec<Type>),
    FnPtr(Box<MethodSig>),
    Pinned(Box<Type>),
    Sentinel,
}

#[derive(Debug, Clone)]
pub struct ArrayShape {
    pub rank: u32,
    pub sizes: Vec<u32>,
    pub lo_bounds: Vec<i32>,
}

#[derive(Debug, Clone)]
pub struct MethodSig {
    pub has_this: bool,
    pub explicit_this: bool,
    pub calling_convention: u8,
    pub generic_param_count: u32,
    pub ret_type: Type,
    pub param_types: Vec<Type>,
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }
    fn read_u8(&mut self) -> Result<u8> {
        let b = *self.data.get(self.pos).ok_or_else(|| Error::InvalidSignature("unexpected end".into()))?;
        self.pos += 1;
        Ok(b)
    }
    fn read_uint(&mut self) -> Result<u32> {
        let (v, n) = decode_compressed_uint(&self.data[self.pos..])?;
        self.pos += n;
        Ok(v as u32)
    }
    fn read_coded_typedeforref(&mut self) -> Result<CodedIndex> {
        let v = self.read_uint()?;
        Ok(decode_coded(Coded::TypeDefOrRef, v))
    }
}

/// Parse a Type (II.23.2.12), skipping custom modifiers.
pub fn parse_type(blob: &[u8]) -> Result<Type> {
    let mut c = Cursor::new(blob);
    let t = read_type(&mut c)?;
    Ok(t)
}

/// Parse a Type and return it together with the number of bytes consumed.
pub fn parse_type_with_len(blob: &[u8]) -> Result<(Type, usize)> {
    let mut c = Cursor::new(blob);
    let t = read_type(&mut c)?;
    Ok((t, c.pos))
}

fn read_type(c: &mut Cursor<'_>) -> Result<Type> {
    // Skip leading custom modifiers.
    loop {
        let b = c.read_u8()?;
        match b {
            et::CMOD_REQD | et::CMOD_OPT => {
                let _ = c.read_coded_typedeforref()?;
                continue;
            }
            _ => return read_type_from(c, b),
        }
    }
}

fn read_type_from(c: &mut Cursor<'_>, b: u8) -> Result<Type> {
    let t = match b {
        et::VOID => Type::Void,
        et::BOOLEAN => Type::Bool,
        et::CHAR => Type::Char,
        et::I1 => Type::I1,
        et::U1 => Type::U1,
        et::I2 => Type::I2,
        et::U2 => Type::U2,
        et::I4 => Type::I4,
        et::U4 => Type::U4,
        et::I8 => Type::I8,
        et::U8 => Type::U8,
        et::R4 => Type::R4,
        et::R8 => Type::R8,
        et::STRING => Type::String,
        et::I => Type::I,
        et::U => Type::U,
        et::OBJECT => Type::Object,
        et::TYPEDBYREF => Type::TypedRef,
        et::PTR => {
            // PTR Type (may be VOID for void*)
            let inner = read_type(c)?;
            Type::Ptr(Box::new(inner))
        }
        et::BYREF => Type::ByRef(Box::new(read_type(c)?)),
        et::SZARRAY => Type::SzArray(Box::new(read_type(c)?)),
        et::ARRAY => {
            let inner = read_type(c)?;
            let rank = c.read_uint()?;
            let num_sizes = c.read_uint()?;
            let mut sizes = Vec::with_capacity(num_sizes as usize);
            for _ in 0..num_sizes {
                sizes.push(c.read_uint()?);
            }
            let num_lo = c.read_uint()?;
            let mut lo_bounds = Vec::with_capacity(num_lo as usize);
            for _ in 0..num_lo {
                let v = c.read_int()?; // compressed int (signed handled below)
                lo_bounds.push(v);
            }
            Type::Array(Box::new(inner), ArrayShape { rank, sizes, lo_bounds })
        }
        et::VALUETYPE => Type::ValueType(c.read_coded_typedeforref()?),
        et::CLASS => Type::Class(c.read_coded_typedeforref()?),
        et::VAR => Type::Var(c.read_uint()?),
        et::MVAR => Type::MVar(c.read_uint()?),
        et::GENERICINST => {
            let kind = c.read_u8()?;
            let base_coded = c.read_coded_typedeforref()?;
            let base = if kind == et::VALUETYPE {
                Type::ValueType(base_coded)
            } else {
                Type::Class(base_coded)
            };
            let argc = c.read_uint()?;
            let mut args = Vec::with_capacity(argc as usize);
            for _ in 0..argc {
                args.push(read_type(c)?);
            }
            Type::GenericInst(Box::new(base), args)
        }
        et::FNPTR => {
            let sig = read_method_sig(c, b)?;
            Type::FnPtr(Box::new(sig))
        }
        et::PINNED => Type::Pinned(Box::new(read_type(c)?)),
        et::SENTINEL => Type::Sentinel,
        other => return Err(Error::InvalidSignature(format!("unknown element type {other:#x}"))),
    };
    Ok(t)
}

impl<'a> Cursor<'a> {
    fn read_int(&mut self) -> Result<i32> {
        let (v, n) = decode_compressed_uint(&self.data[self.pos..])?;
        self.pos += n;
        // Compressed int for array bounds is signed; rotate sign bit.
        let bits = v as u32;
        let rotated = ((bits >> 1) & 0x3FFF_FFFF) as i32;
        let signed = if bits & 1 != 0 { -rotated } else { rotated };
        Ok(signed)
    }
}

/// Parse a method signature. `first` is the already-read leading byte if any.
fn read_method_sig(c: &mut Cursor<'_>, first: u8) -> Result<MethodSig> {
    let cc_byte = if first == et::FNPTR { c.read_u8()? } else { first };
    let has_this = cc_byte & 0x20 != 0;
    let explicit_this = cc_byte & 0x40 != 0;
    let calling_convention = cc_byte & 0x0F;
    let generic = cc_byte & 0x10 != 0;
    let generic_param_count = if generic { c.read_uint()? } else { 0 };
    let param_count = c.read_uint()?;
    let ret_type = read_type(c)?;
    let mut param_types = Vec::with_capacity(param_count as usize);
    for _ in 0..param_count {
        // sentinel may appear for vararg
        let peek = *c.data.get(c.pos).ok_or_else(|| Error::InvalidSignature("param truncated".into()))?;
        if peek == et::SENTINEL {
            c.pos += 1;
            param_types.push(Type::Sentinel);
        }
        param_types.push(read_type(c)?);
    }
    Ok(MethodSig {
        has_this,
        explicit_this,
        calling_convention,
        generic_param_count,
        ret_type,
        param_types,
    })
}

pub fn parse_method_sig(blob: &[u8]) -> Result<MethodSig> {
    let mut c = Cursor::new(blob);
    let first = c.read_u8()?;
    read_method_sig(&mut c, first)
}

pub fn parse_field_sig(blob: &[u8]) -> Result<Type> {
    let mut c = Cursor::new(blob);
    let tag = c.read_u8()?;
    if tag != 0x06 {
        return Err(Error::InvalidSignature(format!("field sig tag {tag:#x} != 0x06")));
    }
    read_type(&mut c)
}

/// Parse a property signature (ECMA-335 II.23.2.5).
/// Layout: 0x08 \[HASTHIS\] ParamCount Type Params...
/// Returns the property type.
pub fn parse_property_sig(blob: &[u8]) -> Result<Type> {
    let mut c = Cursor::new(blob);
    let tag = c.read_u8()?;
    if tag & 0x0F != 0x08 {
        return Err(Error::InvalidSignature(format!("property sig tag {tag:#x} != 0x08")));
    }
    let _param_count = c.read_uint()?;
    read_type(&mut c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vararg_sentinel() {
        // ECMA-335 II.23.2.4 example: `void v(string a, ..., int b)`.
        // CallConv vararg (0x05), ParamCount = 2 (string + int, sentinel not
        // counted), ret void (0x01), string (0x0E), SENTINEL (0x41), int32.
        let blob = [0x05, 0x02, 0x01, 0x0E, 0x41, 0x08];
        let sig = parse_method_sig(&blob).unwrap();
        assert_eq!(sig.calling_convention, 5);
        assert!(matches!(sig.ret_type, Type::Void));
        assert_eq!(sig.param_types.len(), 3);
        assert!(matches!(sig.param_types[0], Type::String));
        assert!(matches!(sig.param_types[1], Type::Sentinel));
        assert!(matches!(sig.param_types[2], Type::I4));
    }

    #[test]
    fn parses_plain_method_sig() {
        // `int Add(int a, int b)`: default call conv, count 2, ret int32.
        let blob = [0x00, 0x02, 0x08, 0x08, 0x08];
        let sig = parse_method_sig(&blob).unwrap();
        assert_eq!(sig.calling_convention, 0);
        assert!(matches!(sig.ret_type, Type::I4));
        assert_eq!(sig.param_types.len(), 2);
        assert!(matches!(sig.param_types[0], Type::I4));
        assert!(matches!(sig.param_types[1], Type::I4));
    }
}
