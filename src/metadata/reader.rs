use crate::error::{Error, Result};
use crate::metadata::signatures::{parse_field_sig, parse_method_sig, parse_type, MethodSig, Type};
use crate::metadata::streams::MetadataRoot;
use crate::metadata::tables::{decode_coded, Coded, CodedIndex, Tables, tbl};
use crate::pe::{CliHeader, PeImage};

/// High-level metadata access.
pub struct Reader<'a> {
    pub pe: &'a PeImage,
    pub root: &'a MetadataRoot,
    pub tables: &'a Tables,
    pub cli: CliHeader,
}

#[derive(Debug, Clone)]
pub struct MethodBody {
    pub code: Vec<u8>,
    pub max_stack: u32,
    pub local_token: u32,
    pub init_locals: bool,
}

impl<'a> Reader<'a> {
    pub fn new(pe: &'a PeImage, root: &'a MetadataRoot, tables: &'a Tables) -> Result<Self> {
        let cli = pe.cli_header()?;
        Ok(Reader { pe, root, tables, cli })
    }

    // ---- Strings ----
    pub fn string(&self, idx: u32) -> String {
        self.root.get_string(idx).unwrap_or_default()
    }

    pub fn blob(&self, idx: u32) -> &[u8] {
        self.root.get_blob(idx).unwrap_or(&[])
    }

    // ---- TypeDef ----
    pub fn type_def_name(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(1))
    }
    pub fn type_def_namespace(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(2))
    }
    /// Extends (base type) coded index.
    pub fn type_def_extends(&self, row: &crate::metadata::tables::Row) -> CodedIndex {
        decode_coded(Coded::TypeDefOrRef, row.col(3))
    }

    // ---- TypeRef ----
    pub fn type_ref_name(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(1))
    }
    pub fn type_ref_namespace(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(2))
    }
    pub fn type_ref_resolution_scope(&self, row: &crate::metadata::tables::Row) -> CodedIndex {
        decode_coded(Coded::ResolutionScope, row.col(0))
    }

    // ---- MethodDef ----
    pub fn method_name(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(3))
    }
    pub fn method_rva(&self, row: &crate::metadata::tables::Row) -> u32 {
        row.col(0)
    }
    // MethodDef layout: [RVA(0), ImplFlags(1), Flags(2), Name(3), Sig(4), ParamList(5)]
    pub fn method_flags(&self, row: &crate::metadata::tables::Row) -> u16 {
        row.col(2) as u16
    }
    pub fn method_impl_flags(&self, row: &crate::metadata::tables::Row) -> u16 {
        row.col(1) as u16
    }
    pub fn method_sig(&self, row: &crate::metadata::tables::Row) -> Result<MethodSig> {
        parse_method_sig(self.blob(row.col(4)))
    }
    pub fn method_param_sequence_end(&self, row: &crate::metadata::tables::Row) -> u32 {
        row.col(5)
    }

    // ---- Field ----
    pub fn field_name(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(1))
    }
    pub fn field_flags(&self, row: &crate::metadata::tables::Row) -> u16 {
        row.col(0) as u16
    }
    pub fn field_type(&self, row: &crate::metadata::tables::Row) -> Result<Type> {
        parse_field_sig(self.blob(row.col(2)))
    }

    /// Look up the Constant table row for a given field (1-based row index).
    /// Returns `(type_code, value_bytes)` where `type_code` is the ECMA-335
    /// II.23.1.16 element type (e.g. 0x08 = int32).
    pub fn constant_for_field(&self, field_row: u32) -> Option<(u8, &[u8])> {
        for r in self.tables.get(tbl::CONSTANT) {
            let parent = decode_coded(Coded::HasConstant, r.col(2));
            if parent.table == Some(tbl::FIELD) && parent.row == field_row {
                let type_code = r.col(0) as u8;
                let blob = self.blob(r.col(3));
                return Some((type_code, blob));
            }
        }
        None
    }

    // ---- Param ----
    pub fn param_name(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(2))
    }
    pub fn param_sequence(&self, row: &crate::metadata::tables::Row) -> u16 {
        row.col(1) as u16
    }
    pub fn param_flags(&self, row: &crate::metadata::tables::Row) -> u16 {
        row.col(0) as u16
    }

    // ---- MemberRef ----
    pub fn member_ref_name(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(1))
    }
    pub fn member_ref_parent(&self, row: &crate::metadata::tables::Row) -> CodedIndex {
        decode_coded(Coded::MemberRefParent, row.col(0))
    }
    pub fn member_ref_sig(&self, row: &crate::metadata::tables::Row) -> Result<MethodSig> {
        parse_method_sig(self.blob(row.col(2)))
    }

    // ---- Assembly ----
    pub fn assembly_name(&self) -> String {
        if let Some(r) = self.tables.get(tbl::ASSEMBLY).first() {
            self.string(r.col(6))
        } else {
            String::new()
        }
    }
    pub fn assembly_ref_name(&self, row: &crate::metadata::tables::Row) -> String {
        self.string(row.col(6))
    }

    // ---- Generic params ----
    pub fn generic_params_for(&self, owner: CodedIndex) -> Vec<(u16, String)> {
        let mut out = Vec::new();
        for r in self.tables.get(tbl::GENERICPARAM) {
            let owner_ci = decode_coded(Coded::TypeOrMethodDef, r.col(2));
            if owner_ci == owner {
                out.push((r.col(0) as u16, self.string(r.col(3))));
            }
        }
        out.sort_by_key(|(n, _)| *n);
        out
    }

    pub fn nested_parent(&self, type_def_row: u32) -> Option<u32> {
        for r in self.tables.get(tbl::NESTEDCLASS) {
            if r.col(0) == type_def_row {
                return Some(r.col(1));
            }
        }
        None
    }

    // ---- Properties ----

    /// Returns property info for a type: (name, type, getter_method_row, setter_method_row).
    /// Method rows are 0-based indices into the METHODDEF table.
    pub fn properties_for_type(&self, type_row: u32) -> Vec<(String, Type, Option<usize>, Option<usize>)> {
        let mut out = Vec::new();
        // Find PropertyMap rows for this type.
        let mut prop_start = 0u32;
        let mut prop_end = 0u32;
        for (i, r) in self.tables.get(tbl::PROPERTYMAP).iter().enumerate() {
            if r.col(0) == type_row {
                prop_start = r.col(1);
                let next = self.tables.get(tbl::PROPERTYMAP).get(i + 1)
                    .map(|r| r.col(1))
                    .unwrap_or_else(|| self.tables.row_count(tbl::PROPERTY) + 1);
                prop_end = next;
                break;
            }
        }
        if prop_start == 0 && prop_end == 0 {
            return out;
        }
        // For each Property row in range, find getter/setter via MethodSemantics.
        for prop_row in prop_start..prop_end {
            let prop = match self.tables.get(tbl::PROPERTY).get(prop_row as usize - 1) {
                Some(r) => r,
                None => continue,
            };
            let name = self.string(prop.col(1));
            let ptype = match crate::metadata::signatures::parse_property_sig(self.blob(prop.col(2))) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let mut getter: Option<usize> = None;
            let mut setter: Option<usize> = None;
            for ms in self.tables.get(tbl::METHODSEMANTICS) {
                let parent = decode_coded(Coded::HasSemantics, ms.col(2));
                if parent.table == Some(tbl::PROPERTY) && parent.row == prop_row {
                    let sem = ms.col(0) as u16;
                    let method_row = ms.col(1) as usize - 1; // 0-based index into METHODDEF
                    if sem & 0x0002 != 0 {
                        getter = Some(method_row);
                    }
                    if sem & 0x0001 != 0 {
                        setter = Some(method_row);
                    }
                }
            }
            out.push((name, ptype, getter, setter));
        }
        out
    }

    // ---- Resolution helpers ----
    pub fn type_def_or_ref_name(&self, ci: CodedIndex) -> String {
        match ci.table {
            Some(tbl::TYPEDEF) => {
                if let Some(r) = self.tables.get(tbl::TYPEDEF).get(ci.row as usize - 1) {
                    let ns = self.type_def_namespace(r);
                    let name = self.type_def_name(r);
                    qualify(ns, name)
                } else {
                    "?".into()
                }
            }
            Some(tbl::TYPEREF) => {
                if let Some(r) = self.tables.get(tbl::TYPEREF).get(ci.row as usize - 1) {
                    let ns = self.type_ref_namespace(r);
                    let name = self.type_ref_name(r);
                    qualify(ns, name)
                } else {
                    "?".into()
                }
            }
            Some(tbl::TYPESPEC) => {
                if let Some(r) = self.tables.get(tbl::TYPESPEC).get(ci.row as usize - 1) {
                    if let Ok(t) = parse_type(self.blob(r.col(0))) {
                        self.type_name(&t)
                    } else {
                        "?".into()
                    }
                } else {
                    "?".into()
                }
            }
            _ => "?".into(),
        }
    }

    /// Render a Type as a C#-ish type name.
    pub fn type_name(&self, t: &Type) -> String {
        match t {
            Type::Void => "void".into(),
            Type::Bool => "bool".into(),
            Type::Char => "char".into(),
            Type::I1 => "sbyte".into(),
            Type::U1 => "byte".into(),
            Type::I2 => "short".into(),
            Type::U2 => "ushort".into(),
            Type::I4 => "int".into(),
            Type::U4 => "uint".into(),
            Type::I8 => "long".into(),
            Type::U8 => "ulong".into(),
            Type::R4 => "float".into(),
            Type::R8 => "double".into(),
            Type::String => "string".into(),
            Type::I => "IntPtr".into(),
            Type::U => "UIntPtr".into(),
            Type::Object => "object".into(),
            Type::TypedRef => "TypedReference".into(),
            Type::Ptr(inner) => format!("{}*", self.type_name(inner)),
            Type::ByRef(inner) => format!("ref {}", self.type_name(inner)),
            Type::SzArray(inner) => format!("{}[]", self.type_name(inner)),
            Type::Array(inner, shape) => {
                let commas = ",".repeat(shape.rank.saturating_sub(1) as usize);
                format!("{}[{}]", self.type_name(inner), commas)
            }
            Type::ValueType(ci) | Type::Class(ci) => self.type_def_or_ref_name(*ci),
            Type::Var(n) => format!("T{}", n),
            Type::MVar(n) => format!("!!{}", n),
            Type::GenericInst(base, args) => {
                let base_name = match base.as_ref() {
                    Type::ValueType(ci) | Type::Class(ci) => self.type_def_or_ref_name(*ci),
                    other => self.type_name(other),
                };
                let args_str = args.iter().map(|a| self.type_name(a)).collect::<Vec<_>>().join(", ");
                format!("{base_name}<{args_str}>")
            }
            Type::FnPtr(_) => "delegate*".into(),
            Type::Pinned(inner) => self.type_name(inner),
            Type::Sentinel => "...".into(),
        }
    }

    /// Param rows (1-based indices) belonging to a method, given its
    /// 1-based MethodDef row index.
    pub fn method_param_rows(&self, method_row: u32) -> std::ops::Range<usize> {
        let methods = self.tables.get(tbl::METHODDEF);
        let start = methods.get(method_row as usize - 1).map(|r| r.col(5) as usize).unwrap_or(0);
        let next = methods.get(method_row as usize).map(|r| r.col(5) as usize).unwrap_or_else(|| self.tables.row_count(tbl::PARAM) as usize + 1);
        start.saturating_sub(1)..next.saturating_sub(1).max(start.saturating_sub(1))
    }

    /// Field rows (0-based indices) belonging to a TypeDef (1-based row).
    pub fn type_field_rows(&self, type_row: u32) -> std::ops::Range<usize> {
        let types = self.tables.get(tbl::TYPEDEF);
        let start = types.get(type_row as usize - 1).map(|r| r.col(4) as usize).unwrap_or(0);
        let next = types.get(type_row as usize).map(|r| r.col(4) as usize).unwrap_or_else(|| self.tables.row_count(tbl::FIELD) as usize + 1);
        start.saturating_sub(1)..next.saturating_sub(1).max(start.saturating_sub(1))
    }

    /// MethodDef rows (0-based indices) belonging to a TypeDef (1-based row).
    pub fn type_method_rows(&self, type_row: u32) -> std::ops::Range<usize> {
        let types = self.tables.get(tbl::TYPEDEF);
        let start = types.get(type_row as usize - 1).map(|r| r.col(5) as usize).unwrap_or(0);
        let next = types.get(type_row as usize).map(|r| r.col(5) as usize).unwrap_or_else(|| self.tables.row_count(tbl::METHODDEF) as usize + 1);
        start.saturating_sub(1)..next.saturating_sub(1).max(start.saturating_sub(1))
    }

    // ---- Method body ----
    pub fn method_body(&self, rva: u32) -> Result<Option<MethodBody>> {
        if rva == 0 {
            return Ok(None);
        }
        let off = self.pe.rva_to_offset(rva)?;
        let d = &self.pe.data;
        if off >= d.len() {
            return Err(Error::InvalidCil(format!("method body rva {rva:#x} out of bounds")));
        }
        let header_byte = d[off];
        if header_byte & 0x03 == 0x02 {
            // Tiny header
            let code_size = (header_byte >> 2) as usize;
            let code_off = off + 1;
            if code_off + code_size > d.len() {
                return Err(Error::InvalidCil("tiny method body truncated".into()));
            }
            Ok(Some(MethodBody {
                code: d[code_off..code_off + code_size].to_vec(),
                max_stack: 8,
                local_token: 0,
                init_locals: false,
            }))
        } else if header_byte & 0x03 == 0x03 {
            // Fat header: 12 bytes
            if off + 12 > d.len() {
                return Err(Error::InvalidCil("fat header truncated".into()));
            }
            let _flags_size = u16::from_le_bytes([d[off], d[off + 1]]);
            let max_stack = u16::from_le_bytes([d[off + 2], d[off + 3]]) as u32;
            let code_size = u32::from_le_bytes([d[off + 4], d[off + 5], d[off + 6], d[off + 7]]) as usize;
            let local_token = u32::from_le_bytes([d[off + 8], d[off + 9], d[off + 10], d[off + 11]]);
            let init_locals = header_byte & 0x10 != 0;
            let code_off = off + 12;
            if code_off + code_size > d.len() {
                return Err(Error::InvalidCil("fat method body truncated".into()));
            }
            Ok(Some(MethodBody {
                code: d[code_off..code_off + code_size].to_vec(),
                max_stack,
                local_token,
                init_locals,
            }))
        } else {
            Err(Error::InvalidCil(format!("unknown method header byte {header_byte:#x}")))
        }
    }

    /// Local variable types from a StandAloneSig token (LOCAL_SIG).
    pub fn local_types(&self, local_token: u32) -> Vec<Type> {
        if local_token == 0 {
            return Vec::new();
        }
        let table = (local_token >> 24) as u8;
        let row = (local_token & 0x00FF_FFFF) as usize;
        if table != tbl::STANDALONESIG {
            return Vec::new();
        }
        let Some(sig_row) = self.tables.get(tbl::STANDALONESIG).get(row - 1) else {
            return Vec::new();
        };
        let blob = self.blob(sig_row.col(0));
        // LOCAL_SIG: 0x07, count, Type*
        if blob.is_empty() || blob[0] != 0x07 {
            return Vec::new();
        }
        let (count, adv) = match crate::metadata::streams::decode_compressed_uint(&blob[1..]) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let mut types = Vec::with_capacity(count);
        let mut pos = 1 + adv;
        for _ in 0..count {
            match crate::metadata::signatures::parse_type_with_len(&blob[pos..]) {
                Ok((t, n)) => {
                    types.push(t);
                    pos += n;
                }
                Err(_) => break,
            }
        }
        types
    }
}

fn qualify(ns: String, name: String) -> String {
    if ns.is_empty() {
        name
    } else {
        format!("{ns}.{name}")
    }
}
