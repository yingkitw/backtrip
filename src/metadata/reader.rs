use crate::error::{Error, Result};
use crate::metadata::signatures::{parse_field_sig, parse_method_sig, parse_type, MethodSig, Type};
use crate::metadata::streams::MetadataRoot;
use crate::metadata::tables::{decode_coded, Coded, CodedIndex, Tables, tbl};
use crate::pe::{CliHeader, PeImage};

/// Map well-known FCL type names to C# keywords and clean up arity suffixes.
/// `System.Int32` → `int`, `System.String` → `string`,
/// `System.Collections.Generic.List`1` → `System.Collections.Generic.List`
fn clean_type_name(full: &str) -> String {
    // Strip backtick arity suffix: `List`1` → `List`
    let without_arity = if let Some(bt) = full.find('`') {
        &full[..bt]
    } else {
        full
    };
    // Map well-known FCL types to C# keywords.
    match without_arity {
        "System.Object" => "object".into(),
        "System.String" => "string".into(),
        "System.Boolean" => "bool".into(),
        "System.Char" => "char".into(),
        "System.SByte" => "sbyte".into(),
        "System.Byte" => "byte".into(),
        "System.Int16" => "short".into(),
        "System.UInt16" => "ushort".into(),
        "System.Int32" => "int".into(),
        "System.UInt32" => "uint".into(),
        "System.Int64" => "long".into(),
        "System.UInt64" => "ulong".into(),
        "System.Single" => "float".into(),
        "System.Double" => "double".into(),
        "System.Void" => "void".into(),
        // Other System.* types render as their simple name; the emitted
        // `using` directives (see `external_namespaces`) resolve them.
        // `System.Collections.Generic.List` → `List`,
        // `System.IO.StreamReader` → `StreamReader`.
        _ if without_arity.starts_with("System.") => {
            without_arity.rsplit('.').next().unwrap_or(without_arity).to_string()
        }
        _ => without_arity.to_string(),
    }
}

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
    pub exceptions: Vec<ExceptionHandler>,
}

#[derive(Debug, Clone)]
pub struct ExceptionHandler {
    pub flags: u32,       // 0=catch, 1=filter, 2=finally, 3=fault
    pub try_offset: u32,
    pub try_length: u32,
    pub handler_offset: u32,
    pub handler_length: u32,
    pub class_token: u32, // TypeRef/TypeDef token for catch type (0 for finally/fault)
    pub filter_offset: u32, // For filter handlers
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

    /// Distinct, sorted namespaces referenced by TypeRef rows (external
    /// types only). Used to emit `using` directives so that stripped simple
    /// names (`List`, `StreamReader`) resolve when the output is recompiled.
    pub fn external_namespaces(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for r in self.tables.get(tbl::TYPEREF) {
            let ns = self.type_ref_namespace(r);
            if !ns.is_empty() {
                set.insert(ns);
            }
        }
        // `[DllImport]` is encoded in the ImplMap table (not a CustomAttribute
        // TypeRef), so the interop namespace is invisible to the scan above.
        if self.tables.row_count(tbl::IMPLMAP) > 0 {
            set.insert("System.Runtime.InteropServices".into());
        }
        set.into_iter().collect()
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

    /// Look up the Constant table row for a given Param (1-based row index).
    /// Returns `(type_code, value_bytes)`.
    pub fn constant_for_param(&self, param_row: u32) -> Option<(u8, &[u8])> {
        for r in self.tables.get(tbl::CONSTANT) {
            let parent = decode_coded(Coded::HasConstant, r.col(2));
            if parent.table == Some(tbl::PARAM) && parent.row == param_row {
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

    // ---- P/Invoke ----

    /// Returns P/Invoke info for a MethodDef row: `(dll_name, import_name)`.
    /// `method_row` is 1-based.
    pub fn pinvoke_info(&self, method_row: u32) -> Option<(String, String)> {
        for im in self.tables.get(tbl::IMPLMAP) {
            let member = decode_coded(Coded::MemberForwarded, im.col(1));
            if member.table == Some(tbl::METHODDEF) && member.row == method_row {
                let import_name = self.string(im.col(2));
                let module_ref = self.tables.get(tbl::MODULEREF).get(im.col(3) as usize - 1)?;
                let dll_name = self.string(module_ref.col(0));
                return Some((dll_name, import_name));
            }
        }
        None
    }

    // ---- Explicit interface implementations ----

    /// Returns explicit interface impls for a type:
    /// `(method_body_0based_row, interface_type_name, interface_method_name)`.
    pub fn explicit_impls_for_type(&self, type_row: u32) -> Vec<(usize, String, String)> {
        let mut out = Vec::new();
        for mi in self.tables.get(tbl::METHODIMPL) {
            if mi.col(0) != type_row {
                continue;
            }
            // MethodBody (col 1): the implementing method (MethodDef or MemberRef).
            let body_ci = decode_coded(Coded::MethodDefOrRef, mi.col(1));
            // MethodDeclaration (col 2): the interface method being implemented.
            let decl_ci = decode_coded(Coded::MethodDefOrRef, mi.col(2));
            // Only handle MethodDef bodies (the implementing method is in this assembly).
            if body_ci.table != Some(tbl::METHODDEF) {
                continue;
            }
            let body_row = body_ci.row as usize - 1; // 0-based
            // Resolve the declaration to get interface type name + method name.
            let (iface_name, method_name) = match decl_ci.table {
                Some(tbl::METHODDEF) => {
                    // Same-assembly interface method.
                    let m = match self.tables.get(tbl::METHODDEF).get(decl_ci.row as usize - 1) {
                        Some(r) => r,
                        None => continue,
                    };
                    let mname = self.method_name(m);
                    // Find the owner type via MethodList range scan.
                    let mut owner_name = String::new();
                    let type_defs = self.tables.get(tbl::TYPEDEF);
                    for (i, td) in type_defs.iter().enumerate() {
                        let start = td.col(5) as u32;
                        let next = type_defs.get(i + 1).map(|r| r.col(5) as u32)
                            .unwrap_or_else(|| self.tables.row_count(tbl::METHODDEF) as u32 + 1);
                        if decl_ci.row >= start && decl_ci.row < next {
                            owner_name = self.type_def_name(td);
                            break;
                        }
                    }
                    (owner_name, mname)
                }
                Some(tbl::MEMBERREF) => {
                    // External interface method (referenced via MemberRef).
                    let mr = match self.tables.get(tbl::MEMBERREF).get(decl_ci.row as usize - 1) {
                        Some(r) => r,
                        None => continue,
                    };
                    let mname = self.member_ref_name(mr);
                    let parent = self.member_ref_parent(mr);
                    let iface = self.type_def_or_ref_name(parent);
                    (iface, mname)
                }
                _ => continue,
            };
            out.push((body_row, iface_name, method_name));
        }
        out
    }

    // ---- Custom attributes ----

    /// Returns attribute type names for a given metadata entity (table, row).
    /// `row` is 1-based. Only the attribute type name is returned; constructor
    /// arguments are not yet parsed.
    pub fn custom_attributes_for(&self, table: u8, row: u32) -> Vec<String> {
        let mut out = Vec::new();
        for ca in self.tables.get(tbl::CUSTOMATTRIBUTE) {
            let parent = decode_coded(Coded::HasCustomAttribute, ca.col(0));
            if parent.table == Some(table) && parent.row == row {
                let ctor_ci = decode_coded(Coded::CustomAttributeType, ca.col(1));
                if let Some(name) = self.attribute_type_name(ctor_ci) {
                    out.push(name);
                }
            }
        }
        out
    }

    /// Returns `(attribute_type_name, formatted_args)` for a given entity.
    /// `formatted_args` is a string like `"\"msg\", true"` or empty if no
    /// constructor arguments.
    pub fn custom_attributes_with_args_for(&self, table: u8, row: u32) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for ca in self.tables.get(tbl::CUSTOMATTRIBUTE) {
            let parent = decode_coded(Coded::HasCustomAttribute, ca.col(0));
            if parent.table != Some(table) || parent.row != row {
                continue;
            }
            let ctor_ci = decode_coded(Coded::CustomAttributeType, ca.col(1));
            let Some(name) = self.attribute_type_name(ctor_ci) else { continue };
            let val = self.blob(ca.col(2));
            let args = self.parse_attr_args(ctor_ci, val);
            out.push((name, args));
        }
        out
    }

    /// Parse the Value blob of a CustomAttribute into a formatted args string.
    /// Format (ECMA-335 II.23.3): prolog (01 00), fixed args per ctor sig,
    /// NumNamed (u16), named args.
    fn parse_attr_args(&self, ctor: CodedIndex, val: &[u8]) -> String {
        if val.len() < 2 || val[0] != 0x01 || val[1] != 0x00 {
            return String::new();
        }
        // Get the constructor's parameter types.
        let param_types = match ctor.table {
            Some(tbl::METHODDEF) => {
                let m = match self.tables.get(tbl::METHODDEF).get(ctor.row as usize - 1) {
                    Some(r) => r,
                    None => return String::new(),
                };
                match self.method_sig(m) {
                    Ok(sig) => sig.param_types,
                    Err(_) => return String::new(),
                }
            }
            Some(tbl::MEMBERREF) => {
                let mr = match self.tables.get(tbl::MEMBERREF).get(ctor.row as usize - 1) {
                    Some(r) => r,
                    None => return String::new(),
                };
                match self.member_ref_sig(mr) {
                    Ok(sig) => sig.param_types,
                    Err(_) => return String::new(),
                }
            }
            _ => return String::new(),
        };

        let mut pos = 2; // skip prolog
        let mut args: Vec<String> = Vec::new();

        for pt in &param_types {
            match self.parse_attr_value(val, &mut pos, pt) {
                Some(s) => args.push(s),
                None => return String::new(),
            }
        }

        // Skip named args (NumNamed u16 + each named arg).
        // We don't render named args yet.

        args.join(", ")
    }

    /// Parse a single attribute argument value from the blob at `pos`.
    fn parse_attr_value(&self, val: &[u8], pos: &mut usize, t: &crate::metadata::signatures::Type) -> Option<String> {
        use crate::metadata::signatures::Type;
        match t {
            Type::String => {
                // SerString: compressed length + UTF-8 bytes. Null = 0xFF.
                if *pos >= val.len() { return None; }
                let b = val[*pos];
                if b == 0xFF {
                    *pos += 1;
                    return Some("null".into());
                }
                let (len, len_bytes) = crate::metadata::streams::decode_compressed_uint(&val[*pos..]).ok()?;
                *pos += len_bytes;
                let end = *pos + len as usize;
                if end > val.len() { return None; }
                let s = std::str::from_utf8(&val[*pos..end]).ok()?;
                *pos = end;
                Some(format!("\"{}\"", s))
            }
            Type::I4 => {
                if *pos + 4 > val.len() { return None; }
                let n = i32::from_le_bytes([val[*pos], val[*pos+1], val[*pos+2], val[*pos+3]]);
                *pos += 4;
                Some(n.to_string())
            }
            Type::I8 => {
                if *pos + 8 > val.len() { return None; }
                let n = i64::from_le_bytes(val[*pos..*pos+8].try_into().ok()?);
                *pos += 8;
                Some(format!("{}L", n))
            }
            Type::Bool => {
                if *pos >= val.len() { return None; }
                let b = val[*pos] != 0;
                *pos += 1;
                Some(b.to_string())
            }
            _ => {
                // Unknown type — skip parsing, return None to signal failure.
                None
            }
        }
    }

    /// Resolve an attribute constructor (MethodDef or MemberRef) to its
    /// declaring type name.
    fn attribute_type_name(&self, ctor: CodedIndex) -> Option<String> {
        match ctor.table {
            Some(tbl::METHODDEF) => {
                // The constructor's declaring type is in the MethodDef's
                // owner TypeDef. Find it by scanning TypeDef MethodList.
                let m_row = ctor.row;
                let type_defs = self.tables.get(tbl::TYPEDEF);
                for (i, td) in type_defs.iter().enumerate() {
                    let start = td.col(5) as u32;
                    let next = type_defs.get(i + 1).map(|r| r.col(5) as u32)
                        .unwrap_or_else(|| self.tables.row_count(tbl::METHODDEF) as u32 + 1);
                    if m_row >= start && m_row < next {
                        let name = self.type_def_name(td);
                        return Some(name);
                    }
                }
                None
            }
            Some(tbl::MEMBERREF) => {
                let mr = self.tables.get(tbl::MEMBERREF).get(ctor.row as usize - 1)?;
                let parent = self.member_ref_parent(mr);
                // MemberRefParent is a TypeRef/TypeDef/TypeSpec.
                match parent.table {
                    Some(tbl::TYPEREF) | Some(tbl::TYPEDEF) | Some(tbl::TYPESPEC) => {
                        Some(self.type_def_or_ref_name(parent))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
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

    // ---- Events ----

    /// Returns event info for a type: (name, event_type_name, add_method_row, remove_method_row).
    /// Method rows are 0-based indices into the METHODDEF table.
    pub fn events_for_type(&self, type_row: u32) -> Vec<(String, String, Option<usize>, Option<usize>)> {
        let mut out = Vec::new();
        // Find EventMap rows for this type.
        let mut evt_start = 0u32;
        let mut evt_end = 0u32;
        for (i, r) in self.tables.get(tbl::EVENTMAP).iter().enumerate() {
            if r.col(0) == type_row {
                evt_start = r.col(1);
                let next = self.tables.get(tbl::EVENTMAP).get(i + 1)
                    .map(|r| r.col(1))
                    .unwrap_or_else(|| self.tables.row_count(tbl::EVENT) + 1);
                evt_end = next;
                break;
            }
        }
        if evt_start == 0 && evt_end == 0 {
            return out;
        }
        // For each Event row in range, find add/remove via MethodSemantics.
        for evt_row in evt_start..evt_end {
            let evt = match self.tables.get(tbl::EVENT).get(evt_row as usize - 1) {
                Some(r) => r,
                None => continue,
            };
            let name = self.string(evt.col(1));
            let evt_type_ci = decode_coded(Coded::TypeDefOrRef, evt.col(2));
            let evt_type_name = self.type_def_or_ref_name(evt_type_ci);
            let mut add: Option<usize> = None;
            let mut remove: Option<usize> = None;
            for ms in self.tables.get(tbl::METHODSEMANTICS) {
                let parent = decode_coded(Coded::HasSemantics, ms.col(2));
                if parent.table == Some(tbl::EVENT) && parent.row == evt_row {
                    let sem = ms.col(0) as u16;
                    let method_row = ms.col(1) as usize - 1;
                    if sem & 0x0008 != 0 {
                        add = Some(method_row);
                    }
                    if sem & 0x0010 != 0 {
                        remove = Some(method_row);
                    }
                }
            }
            out.push((name, evt_type_name, add, remove));
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
                    clean_type_name(&qualify(ns, name))
                } else {
                    "?".into()
                }
            }
            Some(tbl::TYPEREF) => {
                if let Some(r) = self.tables.get(tbl::TYPEREF).get(ci.row as usize - 1) {
                    let ns = self.type_ref_namespace(r);
                    let name = self.type_ref_name(r);
                    clean_type_name(&qualify(ns, name))
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

    /// Render a Type as a C#-ish type name. Generic parameters resolve to
    /// index-based fallbacks (`T{n}` / `!!{n}`) when no context is given.
    pub fn type_name(&self, t: &Type) -> String {
        self.type_name_ctx(t, &[], &[])
    }

    /// Render a Type as a C#-ish type name, resolving generic parameters
    /// against the declaring type's and method's GenericParam names
    /// (`Type::Var(n)` → `class_params[n]`, `Type::MVar(n)` →
    /// `method_params[n]`). Falls back to index-based `T{n}` / `!!{n}` when
    /// the name lists are empty or the index is out of range.
    pub fn type_name_ctx(
        &self,
        t: &Type,
        class_params: &[String],
        method_params: &[String],
    ) -> String {
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
            Type::Ptr(inner) => format!("{}*", self.type_name_ctx(inner, class_params, method_params)),
            Type::ByRef(inner) => format!("ref {}", self.type_name_ctx(inner, class_params, method_params)),
            Type::SzArray(inner) => format!("{}[]", self.type_name_ctx(inner, class_params, method_params)),
            Type::Array(inner, shape) => {
                let commas = ",".repeat(shape.rank.saturating_sub(1) as usize);
                format!("{}[{}]", self.type_name_ctx(inner, class_params, method_params), commas)
            }
            Type::ValueType(ci) | Type::Class(ci) => self.type_def_or_ref_name(*ci),
            Type::Var(n) => class_params
                .get(*n as usize)
                .cloned()
                .unwrap_or_else(|| format!("T{n}")),
            Type::MVar(n) => method_params
                .get(*n as usize)
                .cloned()
                .unwrap_or_else(|| format!("!!{n}")),
            Type::GenericInst(base, args) => {
                let base_name = match base.as_ref() {
                    Type::ValueType(ci) | Type::Class(ci) => self.type_def_or_ref_name(*ci),
                    other => self.type_name_ctx(other, class_params, method_params),
                };
                let args_str = args
                    .iter()
                    .map(|a| self.type_name_ctx(a, class_params, method_params))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{base_name}<{args_str}>")
            }
            Type::FnPtr(_) => "delegate*".into(),
            Type::Pinned(inner) => self.type_name_ctx(inner, class_params, method_params),
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
                exceptions: Vec::new(),
            }))
        } else if header_byte & 0x03 == 0x03 {
            // Fat header: 12 bytes
            if off + 12 > d.len() {
                return Err(Error::InvalidCil("fat header truncated".into()));
            }
            let flags_size = u16::from_le_bytes([d[off], d[off + 1]]);
            let max_stack = u16::from_le_bytes([d[off + 2], d[off + 3]]) as u32;
            let code_size = u32::from_le_bytes([d[off + 4], d[off + 5], d[off + 6], d[off + 7]]) as usize;
            let local_token = u32::from_le_bytes([d[off + 8], d[off + 9], d[off + 10], d[off + 11]]);
            let init_locals = header_byte & 0x10 != 0;
            let more_sections = header_byte & 0x08 != 0;
            let code_off = off + 12;
            if code_off + code_size > d.len() {
                return Err(Error::InvalidCil("fat method body truncated".into()));
            }
            let code = d[code_off..code_off + code_size].to_vec();

            // Parse exception handling sections if present.
            let mut exceptions = Vec::new();
            if more_sections {
                let mut sec_off = code_off + code_size;
                // Align to 4-byte boundary
                sec_off = (sec_off + 3) & !3;
                while sec_off < d.len() {
                    let kind = d[sec_off];
                    let is_fat = kind & 0x40 != 0;
                    let is_ehc = kind & 0x01 != 0;
                    if !is_ehc {
                        break;
                    }
                    let (data_size, clause_size, clauses_off) = if is_fat {
                        // Fat section header: 4 bytes (kind + 3-byte data size)
                        let ds = u32::from_le_bytes([0, d[sec_off + 1], d[sec_off + 2], d[sec_off + 3]]);
                        (ds, 24usize, sec_off + 4)
                    } else {
                        // Small section header: 4 bytes (kind + 3x data size)
                        let ds = d[sec_off + 1] as u32;
                        (ds, 12usize, sec_off + 4)
                    };
                    let clause_count = ((data_size - 4) / clause_size as u32) as usize;
                    for c in 0..clause_count {
                        let co = clauses_off + c * clause_size;
                        if co + clause_size > d.len() {
                            break;
                        }
                        let (flags, try_off, try_len, h_off, h_len, token) = if is_fat {
                            (
                                u32::from_le_bytes([d[co], d[co+1], d[co+2], d[co+3]]),
                                u32::from_le_bytes([d[co+4], d[co+5], d[co+6], d[co+7]]),
                                u32::from_le_bytes([d[co+8], d[co+9], d[co+10], d[co+11]]),
                                u32::from_le_bytes([d[co+12], d[co+13], d[co+14], d[co+15]]),
                                u32::from_le_bytes([d[co+16], d[co+17], d[co+18], d[co+19]]),
                                u32::from_le_bytes([d[co+20], d[co+21], d[co+22], d[co+23]]),
                            )
                        } else {
                            (
                                u16::from_le_bytes([d[co], d[co+1]]) as u32,
                                u16::from_le_bytes([d[co+2], d[co+3]]) as u32,
                                d[co+4] as u32,
                                u16::from_le_bytes([d[co+5], d[co+6]]) as u32,
                                d[co+7] as u32,
                                u32::from_le_bytes([d[co+8], d[co+9], d[co+10], d[co+11]]),
                            )
                        };
                        let filter_offset = if flags == 1 { token } else { 0 };
                        exceptions.push(ExceptionHandler {
                            flags,
                            try_offset: try_off,
                            try_length: try_len,
                            handler_offset: h_off,
                            handler_length: h_len,
                            class_token: if flags == 1 { 0 } else { token },
                            filter_offset,
                        });
                    }
                    // Move to next section
                    sec_off = clauses_off - 4 + data_size as usize;
                    // Align to 4-byte boundary
                    sec_off = (sec_off + 3) & !3;
                    if !is_fat {
                        // Small sections are always the last section
                        break;
                    }
                }
            }

            let _ = flags_size;
            Ok(Some(MethodBody {
                code,
                max_stack,
                local_token,
                init_locals,
                exceptions,
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
