use crate::cil::decoder::{decode, Instruction, Operand};
use crate::error::Result;
use crate::metadata::reader::Reader;
use crate::metadata::signatures::{MethodSig, Type};
use crate::metadata::tables::{Coded, CodedIndex, tbl};

#[derive(Debug, Clone)]
pub struct DecompiledType {
    pub file_name: String,
    pub source: String,
}

/// Decompile every type in the assembly into C# source files.
pub fn decompile_assembly(reader: &Reader<'_>) -> Result<Vec<DecompiledType>> {
    let mut out = Vec::new();
    let type_defs = reader.tables.get(tbl::TYPEDEF);
    for (i, row) in type_defs.iter().enumerate() {
        let row_idx = (i + 1) as u32;
        let name = reader.type_def_name(row);
        // Row 1 is the synthetic <Module> type; skip it.
        if name == "<Module>" {
            continue;
        }
        // Nested types are rendered inside their parent; skip them here.
        if reader.nested_parent(row_idx).is_some() {
            continue;
        }
        let source = decompile_type(reader, row_idx)?;
        let ns = reader.type_def_namespace(row);
        let file_name = file_name_for(&ns, &name);
        out.push(DecompiledType { file_name, source });
    }
    Ok(out)
}

/// Decompile a single type, matched by simple name or fully-qualified name
/// (`Namespace.Name`). Returns `Ok(None)` if no type matches.
pub fn decompile_type_by_name(reader: &Reader<'_>, query: &str) -> Result<Option<DecompiledType>> {
    let type_defs = reader.tables.get(tbl::TYPEDEF);
    for (i, row) in type_defs.iter().enumerate() {
        let row_idx = (i + 1) as u32;
        let name = reader.type_def_name(row);
        if name == "<Module>" {
            continue;
        }
        let ns = reader.type_def_namespace(row);
        let full = if ns.is_empty() {
            name.clone()
        } else {
            format!("{ns}.{name}")
        };
        if name == query || full == query {
            // Nested types are rendered inside their parent; skip standalone.
            if reader.nested_parent(row_idx).is_some() {
                continue;
            }
            let source = decompile_type(reader, row_idx)?;
            let file_name = file_name_for(&ns, &name);
            return Ok(Some(DecompiledType { file_name, source }));
        }
    }
    Ok(None)
}

fn file_name_for(ns: &str, name: &str) -> String {
    // Sanitize nested-type separators and generic arity markers.
    let clean = name.replace('`', "_").replace('/', "_").replace('\\', "_");
    if ns.is_empty() {
        format!("{clean}.cs")
    } else {
        format!("{}_{clean}.cs", ns.replace('.', "_"))
    }
}

fn decompile_type(reader: &Reader<'_>, row_idx: u32) -> Result<String> {
    let row = &reader.tables.get(tbl::TYPEDEF)[row_idx as usize - 1];
    let flags = row.col(0);
    let name = reader.type_def_name(row);
    let ns = reader.type_def_namespace(row);
    let extends = reader.type_def_extends(row);
    let access = type_access(flags);
    let is_interface = flags & 0x00000020 != 0;
    let is_abstract = flags & 0x00000080 != 0;
    let is_sealed = flags & 0x00000100 != 0;

    // Determine kind from the base type: structs extend System.ValueType,
    // enums extend System.Enum; everything else (with a class semantic) is a class.
    let base_full = base_type_name(reader, extends).unwrap_or_default();
    let base_simple = strip_system(&base_full);
    let is_struct = base_simple == "ValueType";
    let is_enum = base_simple == "Enum";
    let is_delegate = base_simple == "MulticastDelegate";
    let kind = if is_interface {
        "interface"
    } else if is_enum {
        "enum"
    } else if is_struct {
        "struct"
    } else {
        "class"
    };

    let generics = reader.generic_params_for(CodedIndex { table: Some(tbl::TYPEDEF), row: row_idx });
    let generic_decl = if generics.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = generics.iter().map(|(_, n)| n.clone()).collect();
        format!("<{}>", names.join(", "))
    };

    let mut s = String::new();
    if !ns.is_empty() {
        s.push_str(&format!("namespace {ns};\n\n"));
    }
    // Custom attributes on the type.
    let type_attrs = reader.custom_attributes_with_args_for(tbl::TYPEDEF, row_idx);
    for (name, args) in &type_attrs {
        s.push_str(&format!("    {}\n", format_attr_line(name, args)));
    }
    let mut keywords = Vec::new();
    if !access.is_empty() {
        keywords.push(access.to_string());
    }
    if is_abstract && !is_interface && !is_struct && !is_delegate {
        keywords.push("abstract".into());
    }
    if is_sealed && !is_interface && !is_struct && !is_enum && !is_delegate {
        keywords.push("sealed".into());
    }
    keywords.push(kind.to_string());

    // Delegates: render as `delegate {ret} {Name}({params});` using the
    // Invoke method's signature. No body, no base type, no fields.
    if is_delegate {
        let method_range = reader.type_method_rows(row_idx);
        let invoke = method_range.clone().find_map(|mi| {
            let m = &reader.tables.get(tbl::METHODDEF)[mi];
            if reader.method_name(m) == "Invoke" {
                reader.method_sig(m).ok().map(|sig| (sig, mi))
            } else {
                None
            }
        });
        if let Some((sig, mi)) = invoke {
            let ret_type = strip_system(&reader.type_name(&sig.ret_type));
            let param_names = method_param_names(reader, (mi + 1) as u32, &sig, false);
            let params: Vec<String> = sig.param_types.iter().enumerate().map(|(i, t)| {
                let pname = param_names.get(i).cloned().unwrap_or_else(|| format!("arg{i}"));
                format!("{} {}", strip_system(&reader.type_name(t)), pname)
            }).collect();
            s.push_str(&format!("{} delegate {} {}{}({});\n",
                access, ret_type, name, generic_decl, params.join(", ")));
            return Ok(s);
        }
        // Fallback: if no Invoke method found, fall through to class rendering.
    }

    s.push_str(&format!("{} {}{}", keywords.join(" "), name, generic_decl));

    // Base type / interfaces.
    let mut bases: Vec<String> = Vec::new();
    if !is_interface && !is_struct && !is_enum {
        if !base_full.is_empty() && base_simple != "Object" && base_simple != "object" {
            bases.push(base_full.clone());
        }
    }
    // Interface implementations.
    for ir in reader.tables.get(tbl::INTERFACEIMPL) {
        if ir.col(0) == row_idx {
            let iface = decode_coded(Coded::TypeDefOrRef, ir.col(1));
            bases.push(simple_name(&strip_system(&reader.type_def_or_ref_name(iface))));
        }
    }
    if !bases.is_empty() {
        s.push_str(&format!(" : {}", bases.join(", ")));
    }
    s.push_str("\n{\n");

    // Fields.
    let field_range = reader.type_field_rows(row_idx);
    let has_fields = !field_range.is_empty();
    if is_enum {
        // Enums: render `enum Name : UnderlyingType { A = 0, B = 1, }`.
        // The `value__` instance field carries the underlying type; the
        // named members are static literal fields with Constant rows.
        let mut underlying = "int".to_string();
        let mut members: Vec<String> = Vec::new();
        for fi in field_range {
            let f = &reader.tables.get(tbl::FIELD)[fi];
            let fname = reader.field_name(f);
            if fname == "value__" {
                if let Ok(t) = reader.field_type(f) {
                    underlying = reader.type_name(&t);
                }
                continue;
            }
            // Named enum member: look up its constant value.
            let value = reader
                .constant_for_field(fi as u32 + 1)
                .map(|(tc, blob)| format_constant(tc, blob))
                .unwrap_or_else(|| "0".into());
            members.push(format!("{fname} = {value}"));
        }
        // Rewrite the header to include the underlying type.
        // `enum Name : int` — replace the already-emitted `enum Name`.
        let header_marker = format!("enum {name}");
        if let Some(pos) = s.rfind(&header_marker) {
            s.replace_range(pos..pos + header_marker.len(), &format!("enum {name} : {underlying}"));
        }
        s.push_str(&format!("    {}\n", members.join(",\n    ")));
        s.push_str("}\n");
        return Ok(s);
    }
    // Collect event names to skip their backing fields.
    let events = reader.events_for_type(row_idx);
    let event_names: std::collections::HashSet<String> = events.iter().map(|(n, _, _, _)| n.clone()).collect();
    for fi in field_range {
        let f = &reader.tables.get(tbl::FIELD)[fi];
        let fname = reader.field_name(f);
        if fname.starts_with("<") && fname.ends_with(">k__BackingField") {
            // Compiler-generated backing field; skip in output.
            continue;
        }
        // Skip event backing fields (private field with same name as the event).
        if event_names.contains(&fname) {
            continue;
        }
        let fflags = reader.field_flags(f);
        let ftype = reader.field_type(f).map(|t| strip_system(&reader.type_name(&t))).unwrap_or_else(|_| "object".into());
        // Field-level custom attributes.
        let field_attrs = reader.custom_attributes_with_args_for(tbl::FIELD, fi as u32 + 1);
        for (name, args) in &field_attrs {
            s.push_str(&format!("    {}\n", format_attr_line(name, args)));
        }
        // Literal + Static flags => C# `const` with a value from the Constant table.
        if fflags & 0x0040 != 0 {
            let value = reader
                .constant_for_field(fi as u32 + 1)
                .map(|(tc, blob)| format_constant(tc, blob))
                .unwrap_or_else(|| "default".into());
            s.push_str(&format!("    {} const {} {} = {};\n", field_access(fflags), ftype, fname, value));
        } else {
            s.push_str(&format!("    {} {} {};\n", field_access(fflags), ftype, fname));
        }
    }
    if has_fields {
        s.push('\n');
    }

    // Properties.
    let properties = reader.properties_for_type(row_idx);
    // Collect property method rows (getter/setter) to skip in the method loop.
    let mut property_methods: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (pname, ptype, getter, setter) in &properties {
        if let Some(g) = getter { property_methods.insert(*g); }
        if let Some(s) = setter { property_methods.insert(*s); }
        let access_str = if let Some(g) = getter {
            // Use the getter's access modifier.
            let m = &reader.tables.get(tbl::METHODDEF)[*g];
            method_access(reader.method_flags(m))
        } else {
            "public"
        };
        let type_str = strip_system(&reader.type_name(ptype));
        let mut accessors: Vec<&str> = Vec::new();
        if getter.is_some() { accessors.push("get"); }
        if setter.is_some() { accessors.push("set"); }
        s.push_str(&format!("    {} {} {} {{ {} }}\n", access_str, type_str, pname, accessors.join("; ") + ";"));
        let _ = pname;
    }
    if !properties.is_empty() {
        s.push('\n');
    }

    // Events.
    let mut event_methods: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (ename, evt_type, add, remove) in &events {
        if let Some(a) = add { event_methods.insert(*a); }
        if let Some(r) = remove { event_methods.insert(*r); }
        let access_str = if let Some(a) = add {
            let m = &reader.tables.get(tbl::METHODDEF)[*a];
            method_access(reader.method_flags(m))
        } else {
            "public"
        };
        s.push_str(&format!("    {} event {} {};\n", access_str, simple_name(&strip_system(evt_type)), ename));
    }
    if !events.is_empty() {
        s.push('\n');
    }

    // Methods (skip property getter/setter and event add/remove methods).
    // Explicit interface impls get a qualified name.
    let explicit_impls = reader.explicit_impls_for_type(row_idx);
    let explicit_map: std::collections::HashMap<usize, (String, String)> =
        explicit_impls.iter().map(|(r, i, m)| (*r, (i.clone(), m.clone()))).collect();
    let method_range = reader.type_method_rows(row_idx);
    for mi in method_range {
        if property_methods.contains(&mi) || event_methods.contains(&mi) {
            continue;
        }
        let m = &reader.tables.get(tbl::METHODDEF)[mi];
        let explicit = explicit_map.get(&mi).map(|(i, m)| (i.as_str(), m.as_str()));
        let src = decompile_method(reader, m, (mi + 1) as u32, &name, explicit)?;
        s.push_str(&src);
        s.push('\n');
    }

    // Nested types: render child types inside the parent's braces.
    let nested_children = nested_types_for(reader, row_idx);
    for child_idx in nested_children {
        let child_src = decompile_nested_type(reader, child_idx)?;
        s.push_str(&child_src);
        s.push('\n');
    }

    s.push_str("}\n");
    Ok(s)
}

/// Find all TypeDef rows whose NestedClass parent is `row_idx`.
fn nested_types_for(reader: &Reader<'_>, row_idx: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let type_defs = reader.tables.get(tbl::TYPEDEF);
    for (i, _row) in type_defs.iter().enumerate() {
        let child_idx = (i + 1) as u32;
        if reader.nested_parent(child_idx) == Some(row_idx) {
            out.push(child_idx);
        }
    }
    out
}

/// Decompile a nested type: same as `decompile_type` but without the
/// namespace header and indented one level for embedding inside the parent.
fn decompile_nested_type(reader: &Reader<'_>, row_idx: u32) -> Result<String> {
    let src = decompile_type(reader, row_idx)?;
    // Strip the namespace header (if any) and indent every remaining line.
    let mut lines = src.lines();
    let mut body = String::new();
    // Skip leading `namespace ...;` line(s) and blank lines.
    let mut started = false;
    for line in &mut lines {
        if !started && (line.starts_with("namespace ") || line.is_empty()) {
            continue;
        }
        started = true;
        if line.is_empty() {
            body.push('\n');
        } else {
            body.push_str("    ");
            body.push_str(line);
            body.push('\n');
        }
    }
    Ok(body)
}

fn base_type_name(reader: &Reader<'_>, extends: CodedIndex) -> Option<String> {
    if extends.table.is_none() {
        return None;
    }
    let n = reader.type_def_or_ref_name(extends);
    // Strip common "System." prefix for readability.
    Some(strip_system(&n))
}

fn strip_system(n: &str) -> String {
    if let Some(rest) = n.strip_prefix("System.") {
        rest.to_string()
    } else {
        n.to_string()
    }
}

/// Return the last segment of a dotted type name (e.g. "Shapes.Notify" → "Notify").
fn simple_name(n: &str) -> String {
    n.rsplit('.').next().unwrap_or(n).to_string()
}

fn decompile_method(reader: &Reader<'_>, m: &crate::metadata::tables::Row, method_row: u32, type_name: &str, explicit: Option<(&str, &str)>) -> Result<String> {
    let flags = reader.method_flags(m);
    let name = reader.method_name(m);
    let sig = reader.method_sig(m)?;
    let is_static = flags & 0x0010 != 0;
    let is_abstract = flags & 0x0400 != 0;
    let is_virtual = flags & 0x0040 != 0;
    let is_newslot = flags & 0x0100 != 0;
    let is_final = flags & 0x0020 != 0;
    let is_ctor = name == ".ctor" || name == ".cctor";
    let is_explicit = explicit.is_some();
    let is_pinvoke = flags & 0x2000 != 0;
    let pinvoke = if is_pinvoke { reader.pinvoke_info(method_row) } else { None };

    let generics = reader.generic_params_for(CodedIndex { table: Some(tbl::METHODDEF), row: method_row });
    let generic_decl = if generics.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = generics.iter().map(|(_, n)| n.clone()).collect();
        format!("<{}>", names.join(", "))
    };

    let mut mods = Vec::new();
    if !is_explicit {
        let acc = method_access(flags);
        if !acc.is_empty() {
            mods.push(acc.to_string());
        }
        if is_static {
            mods.push("static".into());
        }
        if is_abstract {
            mods.push("abstract".into());
        } else if is_virtual && is_newslot {
            mods.push("virtual".into());
        } else if is_virtual && is_final {
            mods.push("override".into());
        } else if is_virtual {
            // virtual without newslot: override
            mods.push("override".into());
        }
    }
    if is_pinvoke {
        mods.push("extern".into());
    }

    let param_names = method_param_names(reader, method_row, &sig, is_static);
    let param_rows = reader.method_param_rows(method_row);
    let param_table = reader.tables.get(tbl::PARAM);
    // Build maps: sequence -> (1-based param row index) for default lookup,
    // and sequence -> is_out (Out flag 0x0008).
    let mut param_defaults: std::collections::HashMap<u16, u32> = std::collections::HashMap::new();
    let mut param_is_out: std::collections::HashSet<u16> = std::collections::HashSet::new();
    for (idx, r) in param_table[param_rows.clone()].iter().enumerate() {
        let flags = reader.param_flags(r);
        let seq = reader.param_sequence(r);
        if flags & 0x1000 != 0 {
            let row_1based = (param_rows.start + idx) as u32 + 1;
            param_defaults.insert(seq, row_1based);
        }
        if flags & 0x0002 != 0 {
            param_is_out.insert(seq);
        }
    }
    let params: Vec<String> = sig
        .param_types
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let pname = param_names.get(i).cloned().unwrap_or_else(|| format!("arg{i}"));
            let seq = (i + 1) as u16;
            // Check for default value.
            if let Some(&param_row) = param_defaults.get(&seq) {
                if let Some((tc, blob)) = reader.constant_for_param(param_row) {
                    let def = format_constant(tc, blob);
                    return format!("{} {} = {}", strip_system(&reader.type_name(t)), pname, def);
                }
            }
            // Check for out parameter (ByRef + Out flag → `out` instead of `ref`).
            if param_is_out.contains(&seq) {
                if let Type::ByRef(inner) = t {
                    return format!("out {} {}", strip_system(&reader.type_name(inner)), pname);
                }
            }
            format!("{} {}", strip_system(&reader.type_name(t)), pname)
        })
        .collect();

    // Constructors render as `TypeName(args)` with no return type.
    // Explicit interface impls render as `void IFoo.Bar(args)`.
    let display_name = if let Some((iface, mname)) = explicit {
        format!("{}.{}", simple_name(&strip_system(iface)), mname)
    } else {
        name.clone()
    };
    let header = if is_ctor {
        format!("    {} {}{}({})",
            mods.join(" "),
            type_name,
            generic_decl,
            params.join(", "),
        )
    } else {
        let ret_type = strip_system(&reader.type_name(&sig.ret_type));
        format!("    {} {} {}{}({})",
            mods.join(" "),
            ret_type,
            display_name,
            generic_decl,
            params.join(", "),
        )
    };

    let rva = reader.method_rva(m);
    let body = reader.method_body(rva)?;

    // Method-level custom attributes.
    let method_attrs = reader.custom_attributes_with_args_for(tbl::METHODDEF, method_row);
    let attr_prefix: String = method_attrs.iter()
        .map(|(n, a)| format!("    {}\n", format_attr_line(n, a)))
        .collect();

    // P/Invoke methods have no body; emit [DllImport] + declaration.
    if is_pinvoke {
        let dll = pinvoke.as_ref().map(|(d, _)| d.as_str()).unwrap_or("unknown");
        let import_name = pinvoke.as_ref().map(|(_, n)| n.as_str()).unwrap_or(&name);
        // If the import name differs from the method name, use EntryPoint.
        let entry = if import_name != name {
            format!(", EntryPoint = \"{import_name}\"")
        } else {
            String::new()
        };
        return Ok(format!("{attr_prefix}    [DllImport(\"{dll}\"{entry})]\n{header};\n"));
    }

    if is_abstract || body.is_none() {
        return Ok(format!("{attr_prefix}{header};\n"));
    }

    let body = body.unwrap();
    let local_types = reader.local_types(body.local_token);
    let body_src = decompile_body(reader, &body.code, &param_names, &local_types, &sig, is_static, &body.exceptions)?;

    let mut s = String::new();
    s.push_str(&attr_prefix);
    s.push_str(&header);
    s.push_str("\n    {\n");
    s.push_str(&body_src);
    s.push_str("    }\n");
    Ok(s)
}

fn method_param_names(
    reader: &Reader<'_>,
    method_row: u32,
    sig: &MethodSig,
    is_static: bool,
) -> Vec<String> {
    let rows = reader.method_param_rows(method_row);
    let params = reader.tables.get(tbl::PARAM);
    // Map sequence (1-based) -> name.
    let mut names: Vec<String> = Vec::with_capacity(sig.param_types.len());
    for i in 0..sig.param_types.len() {
        let seq = (i + 1) as u16;
        let mut found = format!("arg{i}");
        for r in params[rows.clone()].iter() {
            if reader.param_sequence(r) == seq {
                let n = reader.param_name(r);
                if !n.is_empty() {
                    found = n;
                }
                break;
            }
        }
        names.push(found);
    }
    // For instance methods, ldarg.0 is `this`; shift arg indexing.
    let _ = is_static;
    names
}

// ---- Body decompilation (expression-stack machine) ----

fn decompile_body(
    reader: &Reader<'_>,
    code: &[u8],
    param_names: &[String],
    local_types: &[Type],
    sig: &MethodSig,
    is_static: bool,
    exceptions: &[crate::metadata::reader::ExceptionHandler],
) -> Result<String> {
    let instrs = decode(code)?;
    let targets = collect_targets(&instrs);

    let mut stack: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    let local_names = local_type_names(local_types);

    // Map IL offset -> output line index (for inserting try/catch markers).
    let mut offset_to_line: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();

    for ins in &instrs {
        // Emit a label if this offset is a branch target.
        if targets.contains(&ins.offset) {
            offset_to_line.insert(ins.offset, out.len());
            out.push(format!("        Label_{:04X}:", ins.offset));
        }

        if !offset_to_line.contains_key(&ins.offset) {
            offset_to_line.insert(ins.offset, out.len());
        }

        if !handle_instr(reader, ins, &mut stack, &mut out, param_names, &local_names, sig, is_static)? {
            // Unsupported instruction: emit a comment with the raw IL and reset.
            out.push(format!("        // unsupported: {} ({})", ins.name, ins.offset));
            stack.clear();
        }
    }

    // Post-process: insert try/catch markers based on exception handlers.
    if !exceptions.is_empty() {
        insert_exception_markers(reader, &mut out, &offset_to_line, exceptions);
    }

    // Post-process: restructure if/else from conditional branches.
    restructure_if_else(&mut out);

    // Post-process: restructure do-while loops (must run before while).
    restructure_do_while_loops(&mut out);

    // Post-process: restructure while loops from back-edges.
    restructure_while_loops(&mut out);

    // Post-process: convert while loops with init+increment to for loops.
    restructure_for_loops(&mut out);

    // Post-process: reconstruct lock blocks from Monitor.Enter/Exit.
    restructure_locks(&mut out);

    // Post-process: reconstruct using blocks from IDisposable + try/finally.
    restructure_using(&mut out);

    // Post-process: reconstruct foreach from GetEnumerator/MoveNext/Current.
    restructure_foreach(&mut out);

    // Post-process: reconstruct collection initializers from dup+Add patterns.
    restructure_collection_initializers(&mut out);

    let mut s = String::new();
    // Local declarations.
    for (i, lt) in local_types.iter().enumerate() {
        let lname = local_names.get(i).cloned().unwrap_or_else(|| format!("V_{i}"));
        s.push_str(&format!("        {} {} = default;\n", strip_system(&reader.type_name(lt)), lname));
    }
    if !local_types.is_empty() {
        s.push('\n');
    }
    for line in &out {
        if line.is_empty() {
            continue;
        }
        s.push_str(line);
        s.push('\n');
    }
    Ok(s)
}

fn local_type_names(local_types: &[Type]) -> Vec<String> {
    (0..local_types.len()).map(|i| format!("V_{i}")).collect()
}

/// Negate a comparison operator for if/else restructuring.
/// `if (a >= b) goto L;` → `if (a < b) { ... }`
fn negate_cond(cond: &str) -> String {
    // If cond is `(!x)` or `!(x)`, strip the negation.
    let trimmed = cond.trim();
    if trimmed.starts_with("(!") && trimmed.ends_with(')') {
        // `(!x)` → `(x)`
        let inner = &trimmed[2..trimmed.len()-1];
        return format!("({inner})");
    }
    if trimmed.starts_with("!(") && trimmed.ends_with(')') {
        // `!(x)` → `(x)`
        let inner = &trimmed[2..trimmed.len()-1];
        return format!("({inner})");
    }
    // cond is like "(a >= b)" — find the operator and flip it.
    let ops = [(">=", "<"), ("<=", ">"), (">", "<="), ("<", ">="), ("==", "!="), ("!=", "==")];
    for (a, b) in ops {
        if cond.contains(a) {
            return cond.replacen(a, b, 1);
        }
    }
    // No comparison operator found — wrap in `!(...)`.
    format!("!{cond}")
}

/// Post-process the output lines to restructure `if (cond) goto Label;` + block
/// + `Label:` into `if (!cond) { block }`.
fn restructure_if_else(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        // Look for: `        if (cond) goto Label_XXXX;`
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.starts_with("if (") || !trimmed.contains(") goto Label_") {
            i += 1;
            continue;
        }
        // Extract the label name.
        let label = match trimmed.rsplit("goto ").next() {
            Some(s) => s.trim().trim_end_matches(';'),
            None => { i += 1; continue; }
        };
        // Extract the condition (everything between "if (" and ") goto").
        let cond_start = trimmed.find("if (").map(|p| p + 4).unwrap_or(0);
        let cond_end = match trimmed[cond_start..].find(") goto") {
            Some(p) => cond_start + p,
            None => { i += 1; continue; }
        };
        let cond = format!("({})", &trimmed[cond_start..cond_end]);

        // Find the label line (must be after the current line).
        let label_pattern = format!("Label_{}:", label.trim_start_matches("Label_"));
        let label_idx = out[i+1..].iter().position(|l| l.trim() == label_pattern);
        let label_idx = match label_idx {
            Some(p) => i + 1 + p,
            None => { i += 1; continue; }
        };

        // The block between the if-line and the label is the "then" body.
        // Check that the block is non-empty and ends with a return or goto.
        if label_idx <= i + 1 {
            i += 1;
            continue;
        }
        let block_end = label_idx - 1;
        let block_last = out[block_end].trim().to_string();
        if !block_last.starts_with("return") && !block_last.starts_with("goto") {
            i += 1;
            continue;
        }

        // Check for else: if the block ends with `goto Label_YYYY;` and there's
        // another block between the current label and Label_YYYY.
        let has_else = block_last.starts_with("goto ");
        let else_label = if has_else {
            block_last.trim_start_matches("goto ").trim_end_matches(';').to_string()
        } else {
            String::new()
        };

        // Restructure: replace the if-line with `if (!cond) {`
        let neg_cond = negate_cond(&cond);
        out[i] = format!("        if {neg_cond} {{");

        // Indent the block lines by 4 spaces.
        for j in (i+1)..label_idx {
            if !out[j].trim().is_empty() {
                out[j] = format!("    {}", out[j]);
            }
        }

        if has_else {
            // Find the else label.
            let else_pattern = format!("Label_{}:", else_label.trim_start_matches("Label_"));
            let else_idx = out[label_idx+1..].iter().position(|l| l.trim() == else_pattern)
                .map(|p| label_idx + 1 + p);
            if let Some(ei) = else_idx {
                // Replace the current label with `} else {`
                out[label_idx] = "        } else {".to_string();
                // Indent the else block.
                for j in (label_idx+1)..ei {
                    if !out[j].trim().is_empty() {
                        out[j] = format!("    {}", out[j]);
                    }
                }
                // Replace the else label with `}`
                out[ei] = "        }".to_string();
                i = ei + 1;
                continue;
            }
        }

        // No else: replace the label with `}`
        out[label_idx] = "        }".to_string();
        i = label_idx + 1;
    }
}

/// Post-process the output lines to restructure while loops from back-edges.
/// Pattern:
///   goto Label_XXXX;          (jump to condition check)
///   Label_YYYY:               (loop body start)
///   ... loop body ...
///   Label_XXXX:               (loop header / condition check)
///   if (cond) goto Label_YYYY;  (back-edge to loop body)
///   ... after loop ...
///
/// Transforms to:
///   while (!cond) {
///     ... loop body ...
///   }
///   ... after loop ...
fn restructure_while_loops(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        // Look for: `goto Label_XXXX;` (forward jump to loop header)
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.starts_with("goto Label_") {
            i += 1;
            continue;
        }
        let header_label = trimmed.trim_start_matches("goto ").trim_end_matches(';');

        // Find the header label line (must be after current line).
        let header_pattern = format!("Label_{}:", header_label.trim_start_matches("Label_"));
        let header_idx = match out[i+1..].iter().position(|l| l.trim() == header_pattern) {
            Some(p) => i + 1 + p,
            None => { i += 1; continue; }
        };

        // The line after the header should be `if (cond) goto Label_YYYY;` (back-edge).
        if header_idx + 1 >= out.len() {
            i += 1;
            continue;
        }
        let cond_line = out[header_idx + 1].trim().to_string();
        if !cond_line.starts_with("if (") || !cond_line.contains(") goto Label_") {
            i += 1;
            continue;
        }

        // Extract the back-edge target (loop body start label).
        let back_label = match cond_line.rsplit("goto ").next() {
            Some(s) => s.trim().trim_end_matches(';'),
            None => { i += 1; continue; }
        };

        // The back-edge target must be between the initial goto and the header.
        let back_pattern = format!("Label_{}:", back_label.trim_start_matches("Label_"));
        let body_start = match out[i+1..header_idx].iter().position(|l| l.trim() == back_pattern) {
            Some(p) => i + 1 + p,
            None => { i += 1; continue; }
        };

        // Extract the condition.
        let cond_start = cond_line.find("if (").map(|p| p + 4).unwrap_or(0);
        let cond_end = match cond_line[cond_start..].find(") goto") {
            Some(p) => cond_start + p,
            None => { i += 1; continue; }
        };
        let cond = format!("({})", &cond_line[cond_start..cond_end]);

        // Restructure:
        // 1. Replace the initial `goto Label_XXXX;` with `while (cond) {`
        //    (the condition is NOT negated — the back-edge means "continue while true")
        out[i] = format!("        while {cond} {{");

        // 2. Remove the loop body start label (Label_YYYY:)
        out[body_start] = String::new(); // will be filtered out

        // 3. Indent the loop body (between body_start+1 and header_idx)
        for j in (body_start+1)..header_idx {
            if !out[j].trim().is_empty() {
                out[j] = format!("    {}", out[j]);
            }
        }

        // 4. Replace the header label with `}`
        out[header_idx] = "        }".to_string();

        // 5. Remove the back-edge if-line (it's now part of the while)
        out[header_idx + 1] = String::new();

        i = header_idx + 2;
    }
}

/// Post-process the output lines to restructure do-while loops.
/// Pattern:
///   Label_XXXX:               (loop body start)
///   ... loop body ...
///   if (cond) goto Label_XXXX;  (back-edge to same label — no initial goto)
///
/// Transforms to:
///   do {
///     ... loop body ...
///   } while (cond);
fn restructure_do_while_loops(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        // Look for a label line: `Label_XXXX:`
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.starts_with("Label_") || !trimmed.ends_with(':') {
            i += 1;
            continue;
        }
        let label_name = trimmed.trim_end_matches(':');

        // Exclude while loops: if the previous line is `goto Label_YYYY;`
        // (jumping to a different label), this is a while loop header, not do-while.
        if i > 0 {
            let prev = out[i - 1].trim();
            if prev.starts_with("goto Label_") && !prev.contains(&format!("goto {};", label_name)) {
                i += 1;
                continue;
            }
        }

        // Search forward for `if (cond) goto Label_XXXX;` (back-edge to same label).
        let back_pattern = format!("goto {};", label_name);
        let back_idx = out[i+1..].iter().position(|l| l.trim().contains(&back_pattern) && l.trim().starts_with("if ("));
        let back_idx = match back_idx {
            Some(p) => i + 1 + p,
            None => { i += 1; continue; }
        };

        // The back-edge line must be `if (cond) goto Label_XXXX;`
        let back_line = out[back_idx].trim().to_string();
        if !back_line.starts_with("if (") || !back_line.ends_with(';') {
            i += 1;
            continue;
        }

        // Extract the condition.
        let cond_start = back_line.find("if (").map(|p| p + 4).unwrap_or(0);
        let cond_end = match back_line[cond_start..].find(") goto") {
            Some(p) => cond_start + p,
            None => { i += 1; continue; }
        };
        let cond = format!("({})", &back_line[cond_start..cond_end]);

        // Restructure:
        // 1. Replace the label with `do {`
        out[i] = "        do {".to_string();

        // 2. Indent the loop body (between label and back-edge)
        for j in (i+1)..back_idx {
            if !out[j].trim().is_empty() {
                out[j] = format!("    {}", out[j]);
            }
        }

        // 3. Replace the back-edge with `} while (cond);`
        out[back_idx] = format!("        }} while {cond};");

        i = back_idx + 1;
    }
}

/// Post-process: convert `init; while (cond) { body; increment; }` into
/// `for (init; cond; increment) { body; }`.
///
/// Detection criteria:
/// 1. A `while (cond) {` line where cond involves a variable V
/// 2. The previous non-empty line is `V = <init>;`
/// 3. The last non-empty line before the closing `}` is `V = (V <op> <delta>);`
fn restructure_for_loops(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        // Look for `while (cond) {`
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.starts_with("while (") || !trimmed.ends_with('{') {
            i += 1;
            continue;
        }

        // Extract the loop variable from the condition.
        // Condition is like `(V_1 <= n)` — take the first operand.
        let cond_inner = trimmed.trim_start_matches("while (").trim_end_matches(") {");
        let loop_var = match cond_inner.split_whitespace().next() {
            Some(v) => v.trim(),
            None => { i += 1; continue; }
        };

        // Check the previous non-empty line is `V = <init>;`
        let init_idx = if i > 0 {
            (0..i).rev().find(|&j| !out[j].trim().is_empty())
        } else {
            None
        };
        let init_idx = match init_idx {
            Some(idx) => idx,
            None => { i += 1; continue; }
        };
        let init_line = out[init_idx].trim().to_string();
        if !init_line.starts_with(&format!("{loop_var} = ")) || !init_line.ends_with(';') {
            i += 1;
            continue;
        }
        let init_expr = &init_line[format!("{loop_var} = ").len()..].trim_end_matches(';');

        // Find the closing `}` for this while loop.
        let close_idx = match out[i+1..].iter().position(|l| l.trim() == "}") {
            Some(p) => i + 1 + p,
            None => { i += 1; continue; }
        };

        // Find the last non-empty line before `}` — should be the increment.
        let incr_idx = (i+1..close_idx).rev().find(|&j| !out[j].trim().is_empty());
        let incr_idx = match incr_idx {
            Some(idx) => idx,
            None => { i += 1; continue; }
        };
        let incr_line = out[incr_idx].trim().to_string();
        // Increment must be `V = (V <op> <delta>);` or `V = <expr>;` involving V.
        if !incr_line.starts_with(&format!("{loop_var} = ")) || !incr_line.ends_with(';') {
            i += 1; continue;
        }
        let incr_expr = &incr_line[format!("{loop_var} = ").len()..].trim_end_matches(';');

        // All criteria met — restructure as for loop.
        let cond_str = cond_inner;
        out[i] = format!("        for ({loop_var} = {init_expr}; {cond_str}; {loop_var} = {incr_expr}) {{");
        // Remove the init line and increment line.
        out[init_idx] = String::new();
        out[incr_idx] = String::new();
        i = close_idx + 1;
    }
}

/// Post-process: reconstruct `lock (obj) { body }` from `Monitor.Enter`/
/// `Monitor.Exit` patterns wrapped in try/finally.
///
/// Pattern in decompiled output:
///   V_X = <lockobj>;
///   V_Y = 0;
///   try {
///   Threading.Monitor.Enter(V_X, ref V_Y);
///   ... body ...
///   goto Label_ZZZZ; // leave try
///   }
///   finally {
///   if (!V_Y) goto Label_WWWW;
///   Threading.Monitor.Exit(V_X);
///   Label_WWWW:
///   // end finally
///   }
///   Label_ZZZZ:
///   return;
fn restructure_locks(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        // Look for `Threading.Monitor.Enter(...)` or `Monitor.Enter(...)`
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.contains("Monitor.Enter(") {
            i += 1;
            continue;
        }

        // Extract the lock object variable from the Enter call.
        // Pattern: `Threading.Monitor.Enter(V_X, ref V_Y)` or `Monitor.Enter(V_X, ref V_Y)`
        let enter_args = match trimmed.split("Monitor.Enter(").nth(1) {
            Some(s) => s.trim_end_matches(')'),
            None => { i += 1; continue; }
        };
        let parts: Vec<&str> = enter_args.split(", ").collect();
        if parts.len() < 2 {
            i += 1; continue;
        }
        let lock_var = parts[0].trim();

        // The line before should be `try {`
        if i == 0 || out[i - 1].trim() != "try {" {
            i += 1;
            continue;
        }

        // Find the closing `}` of the try block, then the `finally {` line.
        let try_close = match out[i+1..].iter().position(|l| l.trim() == "}") {
            Some(p) => i + 1 + p,
            None => { i += 1; continue; }
        };
        if try_close + 1 >= out.len() || out[try_close + 1].trim() != "finally {" {
            i += 1;
            continue;
        }

        // Find the Monitor.Exit call in the finally block.
        let finally_close = match out[try_close+2..].iter().position(|l| l.trim() == "}") {
            Some(p) => try_close + 2 + p,
            None => { i += 1; continue; }
        };
        let exit_line = out[try_close+2..finally_close].iter()
            .find(|l| l.trim().contains("Monitor.Exit("));
        let exit_line = match exit_line {
            Some(l) => l.trim().to_string(),
            None => { i += 1; continue; }
        };
        // Verify the Exit uses the same lock variable.
        if !exit_line.contains(&format!("Monitor.Exit({lock_var})")) {
            i += 1;
            continue;
        }

        // Find the `try {` line index (it's i-1).
        let try_idx = i - 1;

        // The lock object assignment is before the try block.
        // Find the line `V_X = <lockobj>;` before try_idx.
        let lock_init_idx = (0..try_idx).rev()
            .find(|&j| out[j].trim().starts_with(&format!("{lock_var} = ")));
        let lock_expr = match lock_init_idx {
            Some(idx) => {
                let init = out[idx].trim().to_string();
                let expr = init.trim_start_matches(&format!("{lock_var} = ")).trim_end_matches(';');
                expr.to_string()
            }
            None => lock_var.to_string(),
        };

        // Also remove the `V_Y = 0;` line (the lock-taken flag init).
        let flag_init_idx = (0..try_idx).rev()
            .find(|&j| {
                let t = out[j].trim();
                t.starts_with("V_") && t.ends_with("= 0;") && t.contains(" = ")
            });

        // Collect the body lines (between Enter call and the leave goto).
        let body_start = i + 1;
        let body_end = try_close; // exclusive

        // Restructure:
        // 1. Replace the `try {` line with `lock ({lock_expr}) {`
        out[try_idx] = format!("        lock ({lock_expr}) {{");

        // 2. Remove the Enter call line
        out[i] = String::new();

        // 3. Remove the lock init line (if found)
        if let Some(idx) = lock_init_idx {
            out[idx] = String::new();
        }

        // 4. Remove the flag init line (if found)
        if let Some(idx) = flag_init_idx {
            // Make sure it's not the same as lock_init_idx
            if Some(idx) != lock_init_idx {
                out[idx] = String::new();
            }
        }

        // 5. Remove the leave goto (the last non-empty line before try_close)
        for j in (body_start..body_end).rev() {
            if !out[j].trim().is_empty() {
                if out[j].trim().starts_with("goto ") && out[j].contains("leave try") {
                    out[j] = String::new();
                }
                break;
            }
        }

        // 6. Indent the body lines by 4 spaces
        for j in body_start..body_end {
            if !out[j].trim().is_empty() {
                out[j] = format!("    {}", out[j]);
            }
        }

        // 7. Replace the try close `}` — keep it as `}`
        // (already is `}`)

        // 8. Remove the entire finally block (from `finally {` to its `}`)
        for j in (try_close + 1)..=finally_close {
            out[j] = String::new();
        }

        // 9. Remove the label after the finally (e.g. `Label_ZZZZ:`)
        if finally_close + 1 < out.len() {
            let next = out[finally_close + 1].trim();
            if next.starts_with("Label_") && next.ends_with(':') {
                out[finally_close + 1] = String::new();
            }
        }

        i = finally_close + 2;
    }
}

/// Post-process: reconstruct `using (var x = ...) { body }` from
/// `IDisposable.Dispose` patterns wrapped in try/finally.
///
/// Pattern in decompiled output:
///   V_X = <resource>;
///   try {
///   ... body ...
///   goto Label_ZZZZ; // leave try
///   }
///   finally {
///   if (!V_X) goto Label_WWWW;
///   V_X.Dispose();
///   Label_WWWW:
///   // end finally
///   }
///   Label_ZZZZ:
fn restructure_using(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        // Look for `try {` line
        let line = &out[i];
        if line.trim() != "try {" {
            i += 1;
            continue;
        }

        // The line before should be `V_X = <resource>;` (resource init)
        if i == 0 {
            i += 1;
            continue;
        }
        let init_idx = (0..i).rev().find(|&j| !out[j].trim().is_empty());
        let init_idx = match init_idx {
            Some(idx) => idx,
            None => { i += 1; continue; }
        };
        let init_line = out[init_idx].trim().to_string();
        // Must be `V_X = <expr>;` — extract the variable and expression.
        if !init_line.contains(" = ") || !init_line.ends_with(';') {
            i += 1;
            continue;
        }
        let eq_pos = match init_line.find(" = ") {
            Some(p) => p,
            None => { i += 1; continue; }
        };
        let resource_var = &init_line[..eq_pos];
        let resource_expr = &init_line[eq_pos + 3..].trim_end_matches(';');

        // Find the closing `}` of the try block, then the `finally {` line.
        let try_close = match out[i+1..].iter().position(|l| l.trim() == "}") {
            Some(p) => i + 1 + p,
            None => { i += 1; continue; }
        };
        if try_close + 1 >= out.len() || out[try_close + 1].trim() != "finally {" {
            i += 1;
            continue;
        }

        // Find the finally close `}`.
        let finally_close = match out[try_close+2..].iter().position(|l| l.trim() == "}") {
            Some(p) => try_close + 2 + p,
            None => { i += 1; continue; }
        };

        // Check the finally block for the Dispose pattern:
        // `if (!V_X) goto Label_WWWW;` + `V_X.Dispose();`
        let finally_lines: Vec<&String> = out[try_close+2..finally_close].iter().collect();
        let has_null_check = finally_lines.iter().any(|l| {
            l.trim().starts_with("if (!") && l.trim().contains(&format!("goto ")) && l.contains(resource_var)
        });
        let has_dispose = finally_lines.iter().any(|l| {
            l.trim().contains(&format!("{resource_var}.Dispose()"))
        });

        if !has_null_check || !has_dispose {
            i += 1;
            continue;
        }

        // Collect the body lines (between try { and try close })
        let body_start = i + 1;
        let body_end = try_close; // exclusive

        // Restructure:
        // 1. Replace the `try {` line with `using (V_X = <resource>) {`
        out[i] = format!("        using ({resource_var} = {resource_expr}) {{");

        // 2. Remove the resource init line
        out[init_idx] = String::new();

        // 3. Remove the leave goto (the last non-empty line before try_close)
        for j in (body_start..body_end).rev() {
            if !out[j].trim().is_empty() {
                if out[j].trim().starts_with("goto ") && out[j].contains("leave try") {
                    out[j] = String::new();
                }
                break;
            }
        }

        // 4. Indent the body lines by 4 spaces
        for j in body_start..body_end {
            if !out[j].trim().is_empty() {
                out[j] = format!("    {}", out[j]);
            }
        }

        // 5. Remove the entire finally block
        for j in (try_close + 1)..=finally_close {
            out[j] = String::new();
        }

        // 6. Remove the label after the finally block
        if finally_close + 1 < out.len() {
            let next = out[finally_close + 1].trim();
            if next.starts_with("Label_") && next.ends_with(':') {
                out[finally_close + 1] = String::new();
            }
        }

        i = finally_close + 2;
    }
}

/// Post-process: reconstruct `foreach (var item in collection) { body }`
/// from `GetEnumerator`/`MoveNext`/`Current` patterns.
///
/// Pattern in decompiled output:
///   V_X = <collection>.GetEnumerator();
///   try {
///   while (ref V_X.MoveNext()) {
///   V_Y = ref V_X.get_Current();
///   ... body ...
///   }
///   goto Label_ZZZZ; // leave try
///   }
///   finally {
///   ref V_X.Dispose();
///   // end finally
///   }
///   Label_ZZZZ:
fn restructure_foreach(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        // Look for `V_X = <collection>.GetEnumerator();`
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.ends_with(".GetEnumerator();") {
            i += 1;
            continue;
        }
        // Extract the enumerator variable and collection expression.
        let eq_pos = match trimmed.find(" = ") {
            Some(p) => p,
            None => { i += 1; continue; }
        };
        let enum_var = &trimmed[..eq_pos];
        let collection = trimmed[eq_pos + 3..].trim_end_matches(".GetEnumerator();");

        // The next non-empty line should be `try {`
        let try_idx = (i+1..out.len()).find(|&j| !out[j].trim().is_empty());
        let try_idx = match try_idx {
            Some(idx) if out[idx].trim() == "try {" => idx,
            _ => { i += 1; continue; }
        };

        // Inside the try, find `while (ref V_X.MoveNext()) {`
        let while_idx = (try_idx+1..out.len()).find(|&j| {
            let t = out[j].trim();
            t.starts_with("while (") && t.contains(&format!("{enum_var}.MoveNext()")) && t.ends_with("{")
        });
        let while_idx = match while_idx {
            Some(idx) => idx,
            None => { i += 1; continue; }
        };

        // The first non-empty line inside the while should be
        // `V_Y = ref V_X.get_Current();`
        let current_idx = (while_idx+1..out.len()).find(|&j| !out[j].trim().is_empty());
        let (current_idx, item_var) = match current_idx {
            Some(idx) => {
                let t = out[idx].trim();
                if t.contains(&format!("{enum_var}.get_Current()")) && t.contains(" = ") {
                    // Extract V_Y from `V_Y = ref V_X.get_Current();`
                    let eq = t.find(" = ").unwrap();
                    let item = t[..eq].to_string();
                    (idx, item)
                } else {
                    { i += 1; continue; }
                }
            }
            None => { i += 1; continue; }
        };

        // Find the while close `}`
        let while_close = match out[while_idx+1..].iter().position(|l| l.trim() == "}") {
            Some(p) => while_idx + 1 + p,
            None => { i += 1; continue; }
        };

        // Find the try close `}` and `finally {`
        let try_close = match out[while_close+1..].iter().position(|l| l.trim() == "}") {
            Some(p) => while_close + 1 + p,
            None => { i += 1; continue; }
        };
        if try_close + 1 >= out.len() || out[try_close + 1].trim() != "finally {" {
            i += 1;
            continue;
        }

        // Find the finally close `}`
        let finally_close = match out[try_close+2..].iter().position(|l| l.trim() == "}") {
            Some(p) => try_close + 2 + p,
            None => { i += 1; continue; }
        };

        // Verify the finally block contains V_X.Dispose()
        let has_dispose = out[try_close+2..finally_close].iter()
            .any(|l| l.trim().contains(&format!("{enum_var}.Dispose()")));
        if !has_dispose {
            i += 1;
            continue;
        }

        // Restructure:
        // 1. Replace the GetEnumerator init line with `foreach (var V_Y in <collection>) {`
        out[i] = format!("        foreach (var {item_var} in {collection}) {{");

        // 2. Remove the `try {` line
        out[try_idx] = String::new();

        // 3. Remove the `while (...) {` line
        out[while_idx] = String::new();

        // 4. Remove the `V_Y = ref V_X.get_Current();` line
        out[current_idx] = String::new();

        // 5. Remove the while close `}`
        out[while_close] = String::new();

        // 6. Indent the body lines (between current_idx+1 and while_close)
        for j in (current_idx+1)..while_close {
            if !out[j].trim().is_empty() {
                // Remove extra indentation from the while body (it was indented by while)
                // and re-indent for foreach
                let stripped = out[j].trim_start();
                out[j] = format!("            {}", stripped);
            }
        }

        // 7. Remove the leave goto (between while_close and try_close)
        for j in (while_close+1)..try_close {
            if !out[j].trim().is_empty() && out[j].trim().starts_with("goto ") {
                out[j] = String::new();
            }
        }

        // 8. Replace the try close `}` with `}` (foreach close)
        // (already is `}`)

        // 9. Remove the entire finally block
        for j in (try_close + 1)..=finally_close {
            out[j] = String::new();
        }

        // 10. Remove the label after the finally block
        if finally_close + 1 < out.len() {
            let next = out[finally_close + 1].trim();
            if next.starts_with("Label_") && next.ends_with(':') {
                out[finally_close + 1] = String::new();
            }
        }

        i = finally_close + 2;
    }
}

/// Post-process: reconstruct collection initializers from dup+Add patterns.
/// Pattern:
///   V_tmp_N = new Type();
///   V_tmp_N.Add(x);
///   V_tmp_N.Add(y);
///   ... (possibly return V_tmp_N; or V_tmp_N = ...)
///
/// Transforms to:
///   new Type() { x, y, ... }
/// (and removes the Add lines, replacing the temp usage with the initializer)
fn restructure_collection_initializers(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        // Look for `V_tmp_N = new Type();`
        let line = out[i].clone();
        let trimmed = line.trim();
        if !trimmed.starts_with("V_tmp_") || !trimmed.contains(" = new ") || !trimmed.ends_with("();") {
            i += 1;
            continue;
        }

        // Extract the temp variable name and the constructor expression.
        let eq_pos = trimmed.find(" = ").unwrap();
        let temp_var = &trimmed[..eq_pos];
        let ctor_expr = &trimmed[eq_pos + 3..].trim_end_matches(';');

        // Collect Add calls: `V_tmp_N.Add(arg);`
        let mut add_args: Vec<String> = Vec::new();
        let mut j = i + 1;
        while j < out.len() {
            let t = out[j].trim();
            if t == &format!("{temp_var}.Add();") {
                break;
            }
            let prefix = format!("{temp_var}.Add(");
            if t.starts_with(&prefix) && t.ends_with(");") {
                let arg = &t[prefix.len()..].trim_end_matches(");");
                add_args.push(arg.to_string());
                j += 1;
            } else if t.is_empty() {
                j += 1;
            } else {
                break;
            }
        }

        if add_args.is_empty() {
            i += 1;
            continue;
        }

        // Build the collection initializer expression.
        let initializer = format!("{ctor_expr} {{ {} }}", add_args.join(", "));

        // Replace the init line with the initializer (as a non-statement expression).
        // We'll mark it so the next usage can pick it up.
        out[i] = format!("        {temp_var} = {initializer};");

        // Remove the Add lines.
        for k in (i+1)..j {
            if out[k].trim().starts_with(&format!("{temp_var}.Add(")) {
                out[k] = String::new();
            }
        }

        i = j;
    }

    // Second pass: replace `return V_tmp_N;` and `V_X = V_tmp_N;` with the
    // initializer expression directly, and remove the temp declaration.
    // Actually, let's do a simpler approach: just leave the temp assignment
    // with the initializer. The output `V_tmp_0 = new List<int>() { 1, 2, 3 };`
    // is acceptable. But we can clean it up by inlining if the next use is
    // a return or assignment.
    //
    // For now, the temp variable approach is clean enough.
}

/// Clean up compiler-generated field names.
/// `<Count>k__BackingField` → `Count`
fn clean_field_name(fname: &str) -> String {
    if fname.starts_with('<') {
        if let Some(end) = fname.find('>') {
            return fname[1..end].to_string();
        }
    }
    fname.to_string()
}

/// Format a custom attribute line: `[Attr]` or `[Attr("args")]`.
fn format_attr_line(name: &str, args: &str) -> String {
    let simple = simple_name(&strip_system(name));
    let short = simple.strip_suffix("Attribute").unwrap_or(&simple);
    if args.is_empty() {
        format!("[{short}]")
    } else {
        format!("[{short}({args})]")
    }
}

/// Insert `try { ... } catch (...) { ... }` markers into the output lines
/// based on exception handler clauses. This is a simple structural insertion
/// that wraps the try and handler regions.
fn insert_exception_markers(
    reader: &Reader<'_>,
    out: &mut Vec<String>,
    offset_to_line: &std::collections::HashMap<usize, usize>,
    exceptions: &[crate::metadata::reader::ExceptionHandler],
) {
    // Sort by try_offset so we insert from the end backward (to preserve indices).
    let mut sorted = exceptions.to_vec();
    sorted.sort_by_key(|e| (e.try_offset, e.handler_offset));

    // Process in reverse order so insertions don't invalidate earlier indices.
    for eh in sorted.iter().rev() {
        let try_start = *offset_to_line.get(&(eh.try_offset as usize)).unwrap_or(&0);
        let handler_start = *offset_to_line.get(&(eh.handler_offset as usize)).unwrap_or(&0);
        let _try_end = *offset_to_line.get(&((eh.try_offset + eh.try_length) as usize)).unwrap_or(&handler_start);
        let handler_end = *offset_to_line.get(&((eh.handler_offset + eh.handler_length) as usize)).unwrap_or(&out.len());

        // Build the catch clause header.
        let catch_header = match eh.flags {
            0 => {
                // catch: resolve the exception type from the class token.
                let type_name = token_type_name(reader, eh.class_token);
                format!("        catch ({}) {{", simple_name(&strip_system(&type_name)))
            }
            2 => "        finally {".to_string(),
            3 => "        fault {".to_string(),
            1 => "        catch (/* filter */) {".to_string(),
            _ => "        catch {".to_string(),
        };

        // Insert closing brace after handler region.
        if handler_end <= out.len() {
            out.insert(handler_end, "        }".to_string());
        }
        // Insert catch header + closing brace of try before handler region.
        if handler_start <= out.len() {
            out.insert(handler_start, catch_header);
            out.insert(handler_start, "        }".to_string());
        }
        // Insert try { before try region.
        if try_start <= out.len() {
            out.insert(try_start, "        try {".to_string());
        }
    }
}

/// Resolve a metadata token (0x02xxxxxx = TypeDef, 0x01xxxxxx = TypeRef) to a type name.
fn token_type_name(reader: &Reader<'_>, token: u32) -> String {
    let table = (token >> 24) as u8;
    let row = (token & 0x00FF_FFFF) as usize;
    match table {
        tbl::TYPEREF => {
            if let Some(r) = reader.tables.get(tbl::TYPEREF).get(row - 1) {
                let ns = reader.type_ref_namespace(r);
                let name = reader.type_ref_name(r);
                if ns.is_empty() { name } else { format!("{ns}.{name}") }
            } else { "?".into() }
        }
        tbl::TYPEDEF => {
            if let Some(r) = reader.tables.get(tbl::TYPEDEF).get(row - 1) {
                let ns = reader.type_def_namespace(r);
                let name = reader.type_def_name(r);
                if ns.is_empty() { name } else { format!("{ns}.{name}") }
            } else { "?".into() }
        }
        _ => "?".into(),
    }
}

fn collect_targets(instrs: &[Instruction]) -> std::collections::HashSet<usize> {
    let mut set = std::collections::HashSet::new();
    for ins in instrs {
        match &ins.operand {
            Operand::BrTarget(o) => {
                set.insert(target_offset(ins.offset, ins.size, *o as i32));
            }
            Operand::ShortBrTarget(o) => {
                set.insert(target_offset(ins.offset, ins.size, *o as i32));
            }
            Operand::Switch(ts) => {
                let base = ins.offset + ins.size;
                for t in ts {
                    set.insert(target_offset_from(base, *t));
                }
            }
            _ => {}
        }
    }
    set
}

/// Compute an absolute branch target from a relative offset, using signed math.
fn target_offset(instr_offset: usize, instr_size: usize, rel: i32) -> usize {
    let base = instr_offset as i64 + instr_size as i64;
    (base + rel as i64).max(0) as usize
}

fn target_offset_from(base: usize, rel: i32) -> usize {
    (base as i64 + rel as i64).max(0) as usize
}

fn arg_name(param_names: &[String], is_static: bool, idx: u32) -> String {
    if !is_static && idx == 0 {
        return "this".into();
    }
    let real = if is_static { idx } else { idx - 1 };
    param_names
        .get(real as usize)
        .cloned()
        .unwrap_or_else(|| format!("arg_{real}"))
}

/// Handle one instruction. Returns Ok(true) if handled, Ok(false) if unsupported.
fn handle_instr(
    reader: &Reader<'_>,
    ins: &Instruction,
    stack: &mut Vec<String>,
    out: &mut Vec<String>,
    param_names: &[String],
    local_names: &[String],
    sig: &MethodSig,
    is_static: bool,
) -> Result<bool> {
    let name = ins.name;
    let push = |stack: &mut Vec<String>, v: String| stack.push(v);
    let pop = |stack: &mut Vec<String>| stack.pop().unwrap_or_else(|| "/*?*/".into());
    let stmt = |out: &mut Vec<String>, s: String| out.push(format!("        {s}"));

    match name {
        "nop" | "break" => {}
        "ret" => {
            if matches!(sig.ret_type, Type::Void) && stack.is_empty() {
                stmt(out, "return;".into());
            } else if !stack.is_empty() {
                let v = pop(stack);
                stmt(out, format!("return {v};"));
            } else {
                stmt(out, "return;".into());
            }
        }
        "ldnull" => push(stack, "null".into()),
        "dup" => {
            if let Some(top) = stack.last().cloned() {
                // If the top is a `new ...()` expression, introduce a temp
                // variable to avoid rendering the constructor multiple times.
                if top.starts_with("new ") && top.ends_with(")") {
                    let temp = format!("V_tmp_{}", out.len());
                    stmt(out, format!("{temp} = {top};"));
                    // Replace the stack top with the temp variable.
                    if let Some(t) = stack.last_mut() {
                        *t = temp.clone();
                    }
                    push(stack, temp);
                } else {
                    push(stack, top);
                }
            }
        }
        "pop" => {
            let _ = pop(stack);
        }

        // Constants
        "ldc.i4.m1" => push(stack, "-1".into()),
        "ldc.i4.0" => push(stack, "0".into()),
        "ldc.i4.1" => push(stack, "1".into()),
        "ldc.i4.2" => push(stack, "2".into()),
        "ldc.i4.3" => push(stack, "3".into()),
        "ldc.i4.4" => push(stack, "4".into()),
        "ldc.i4.5" => push(stack, "5".into()),
        "ldc.i4.6" => push(stack, "6".into()),
        "ldc.i4.7" => push(stack, "7".into()),
        "ldc.i4.8" => push(stack, "8".into()),
        "ldc.i4.s" => {
            if let Operand::I8(v) = &ins.operand { push(stack, v.to_string()); }
        }
        "ldc.i4" => {
            if let Operand::I32(v) = &ins.operand { push(stack, v.to_string()); }
        }
        "ldc.i8" => {
            if let Operand::I64(v) = &ins.operand { push(stack, format!("{v}L")); }
        }
        "ldc.r4" => {
            if let Operand::R4(v) = &ins.operand { push(stack, format!("{v}f")); }
        }
        "ldc.r8" => {
            if let Operand::R8(v) = &ins.operand { push(stack, format!("{v}")); }
        }

        // Arguments
        "ldarg.0" => push(stack, arg_name(param_names, is_static, 0)),
        "ldarg.1" => push(stack, arg_name(param_names, is_static, 1)),
        "ldarg.2" => push(stack, arg_name(param_names, is_static, 2)),
        "ldarg.3" => push(stack, arg_name(param_names, is_static, 3)),
        "ldarg.s" | "ldarg" => {
            let idx = var_index(&ins.operand);
            push(stack, arg_name(param_names, is_static, idx));
        }
        "ldarga.s" | "ldarga" => {
            let idx = var_index(&ins.operand);
            push(stack, format!("ref {}", arg_name(param_names, is_static, idx)));
        }
        "starg.s" | "starg" => {
            let idx = var_index(&ins.operand);
            let v = pop(stack);
            stmt(out, format!("{} = {v};", arg_name(param_names, is_static, idx)));
        }

        // Locals
        "ldloc.0" => push(stack, local_name(local_names, 0)),
        "ldloc.1" => push(stack, local_name(local_names, 1)),
        "ldloc.2" => push(stack, local_name(local_names, 2)),
        "ldloc.3" => push(stack, local_name(local_names, 3)),
        "ldloc.s" | "ldloc" => {
            let idx = var_index(&ins.operand);
            push(stack, local_name(local_names, idx as usize));
        }
        "ldloca.s" | "ldloca" => {
            let idx = var_index(&ins.operand);
            // Load the address of a local — for method calls on value types,
            // C# just uses the variable name directly (the address is implicit).
            push(stack, local_name(local_names, idx as usize));
        }
        "stloc.0" => store_local(out, stack, local_names, 0),
        "stloc.1" => store_local(out, stack, local_names, 1),
        "stloc.2" => store_local(out, stack, local_names, 2),
        "stloc.3" => store_local(out, stack, local_names, 3),
        "stloc.s" | "stloc" => {
            let idx = var_index(&ins.operand);
            store_local(out, stack, local_names, idx as usize);
        }

        // Arithmetic (binary)
        "add" => binop(stack, "+"),
        "sub" => binop(stack, "-"),
        "mul" => binop(stack, "*"),
        "div" => binop(stack, "/"),
        "div.un" => binop(stack, "/"),
        "rem" => binop(stack, "%"),
        "rem.un" => binop(stack, "%"),
        "and" => binop(stack, "&"),
        "or" => binop(stack, "|"),
        "xor" => binop(stack, "^"),
        "shl" => binop(stack, "<<"),
        "shr" => binop(stack, ">>"),
        "shr.un" => binop(stack, ">>"),
        "add.ovf" => binop(stack, "+"),
        "add.ovf.un" => binop(stack, "+"),
        "sub.ovf" => binop(stack, "-"),
        "sub.ovf.un" => binop(stack, "-"),
        "mul.ovf" => binop(stack, "*"),
        "mul.ovf.un" => binop(stack, "*"),

        // Unary
        "neg" => unop(stack, "-"),
        "not" => unop(stack, "~"),

        // Conversions
        "conv.i1" => conv(stack, "sbyte"),
        "conv.i2" => conv(stack, "short"),
        "conv.i4" => conv(stack, "int"),
        "conv.i8" => conv(stack, "long"),
        "conv.u1" => conv(stack, "byte"),
        "conv.u2" => conv(stack, "ushort"),
        "conv.u4" => conv(stack, "uint"),
        "conv.u8" => conv(stack, "ulong"),
        "conv.r4" => conv(stack, "float"),
        "conv.r8" => conv(stack, "double"),
        "conv.i" => conv(stack, "IntPtr"),
        "conv.u" => conv(stack, "UIntPtr"),
        "conv.r.un" => conv(stack, "double"),

        // Strings
        "ldstr" => {
            if let Operand::Token(tok) = &ins.operand {
                let idx = tok & 0x00FF_FFFF;
                let s = reader.root.get_user_string(idx).unwrap_or_default();
                push(stack, quote_string(&s));
            }
        }

        // Calls
        "call" | "callvirt" => {
            if let Operand::Token(tok) = &ins.operand {
                let (owner, mname, csig) = match resolve_method(reader, *tok) {
                    Some(v) => v,
                    None => return Ok(false),
                };
                let argc = csig.param_types.len();
                let mut args: Vec<String> = Vec::with_capacity(argc);
                for _ in 0..argc {
                    args.push(pop(stack));
                }
                args.reverse();
                let expr = if csig.has_this {
                    let obj = pop(stack);
                    if mname == ".ctor" {
                        // Suppress the implicit base() call to System.Object —
                        // C# emits it implicitly for any class without an
                        // explicit base constructor call.
                        if owner != "Object" && owner != "object" {
                            stmt(out, "base();".into());
                        }
                        String::new()
                    } else {
                        format!("{obj}.{mname}({})", args.join(", "))
                    }
                } else if (owner == "String" || owner == "string") && mname == "Concat" {
                    // String.Concat(a, b, c, ...) → a + b + c
                    args.join(" + ")
                } else {
                    format!("{owner}.{mname}({})", args.join(", "))
                };
                if expr.is_empty() {
                    // base ctor already emitted as statement.
                } else if matches!(csig.ret_type, Type::Void) {
                    stmt(out, format!("{expr};"));
                } else {
                    push(stack, expr);
                }
            } else {
                return Ok(false);
            }
        }
        "newobj" => {
            if let Operand::Token(tok) = &ins.operand {
                let expr = build_newobj(reader, *tok, stack);
                push(stack, expr);
            } else {
                return Ok(false);
            }
        }

        // Fields
        "ldfld" | "ldflda" => {
            if let Operand::Token(tok) = &ins.operand {
                let (tname, fname) = field_ref(reader, *tok);
                let obj = pop(stack);
                push(stack, format!("{obj}.{}", clean_field_name(&fname)));
                let _ = tname;
            }
        }
        "ldsfld" | "ldsflda" => {
            if let Operand::Token(tok) = &ins.operand {
                let (tname, fname) = field_ref(reader, *tok);
                push(stack, format!("{tname}.{}", clean_field_name(&fname)));
            }
        }
        "stfld" => {
            if let Operand::Token(tok) = &ins.operand {
                let (_, fname) = field_ref(reader, *tok);
                let val = pop(stack);
                let obj = pop(stack);
                stmt(out, format!("{obj}.{} = {val};", clean_field_name(&fname)));
            }
        }
        "stsfld" => {
            if let Operand::Token(tok) = &ins.operand {
                let (tname, fname) = field_ref(reader, *tok);
                let val = pop(stack);
                stmt(out, format!("{tname}.{fname} = {val};"));
            }
        }
        // Indirect load: pop a managed pointer, push the dereferenced value.
        "ldind.i4" | "ldind.i8" | "ldind.r4" | "ldind.r8"
        | "ldind.i" | "ldind.u" | "ldind.ref"
        | "ldind.i1" | "ldind.u1" | "ldind.i2" | "ldind.u2"
        | "ldind.u4" => {
            let addr = pop(stack);
            push(stack, format!("*{addr}"));
        }
        // Indirect store: pop a value and a managed pointer, store through.
        "stind.i4" | "stind.i8" | "stind.r4" | "stind.r8"
        | "stind.i" | "stind.ref"
        | "stind.i1" | "stind.i2" => {
            let val = pop(stack);
            let addr = pop(stack);
            stmt(out, format!("*{addr} = {val};"));
        }

        // Object model
        "box" => {
            if let Operand::Token(tok) = &ins.operand {
                let t = type_token_name(reader, *tok);
                let v = pop(stack);
                push(stack, format!("(object)({v})"));
                let _ = t;
            }
        }
        "unbox.any" => {
            if let Operand::Token(tok) = &ins.operand {
                let t = type_token_name(reader, *tok);
                let v = pop(stack);
                push(stack, format!("({t})({v})"));
            }
        }
        "castclass" => {
            if let Operand::Token(tok) = &ins.operand {
                let t = type_token_name(reader, *tok);
                let v = pop(stack);
                let v_s = strip_outer_parens(&v);
                push(stack, format!("({t})({v_s})"));
            }
        }
        "isinst" => {
            if let Operand::Token(tok) = &ins.operand {
                let t = type_token_name(reader, *tok);
                let v = pop(stack);
                let v_s = strip_outer_parens(&v);
                push(stack, format!("{v_s} as {t}"));
            }
        }
        "newarr" => {
            if let Operand::Token(tok) = &ins.operand {
                let t = type_token_name(reader, *tok);
                let count = pop(stack);
                push(stack, format!("new {t}[{count}]"));
            }
        }
        "ldlen" => {
            let v = pop(stack);
            push(stack, format!("{v}.Length"));
        }
        "ldelem.i1" | "ldelem.u1" | "ldelem.i2" | "ldelem.u2" | "ldelem.i4" | "ldelem.u4"
        | "ldelem.i8" | "ldelem.i" | "ldelem.r4" | "ldelem.r8" | "ldelem.ref" | "ldelem" => {
            let idx = pop(stack);
            let arr = pop(stack);
            push(stack, format!("{arr}[{idx}]"));
        }
        "stelem.i" | "stelem.i1" | "stelem.i2" | "stelem.i4" | "stelem.i8"
        | "stelem.r4" | "stelem.r8" | "stelem.ref" | "stelem" => {
            let val = pop(stack);
            let idx = pop(stack);
            let arr = pop(stack);
            stmt(out, format!("{arr}[{idx}] = {val};"));
        }
        "throw" => {
            let v = pop(stack);
            stmt(out, format!("throw {v};"));
        }
        "rethrow" => stmt(out, "throw;".into()),

        // Branches
        "br" | "br.s" => {
            let tgt = branch_target_of(ins);
            stmt(out, format!("goto Label_{tgt:04X};"));
        }
        "leave" | "leave.s" => {
            let tgt = branch_target_of(ins);
            stmt(out, format!("goto Label_{tgt:04X}; // leave try"));
        }
        "brfalse" | "brfalse.s" => {
            let tgt = branch_target_of(ins);
            let v = pop(stack);
            stmt(out, format!("if (!{v}) goto Label_{tgt:04X};"));
        }
        "brtrue" | "brtrue.s" => {
            let tgt = branch_target_of(ins);
            let v = pop(stack);
            stmt(out, format!("if ({v}) goto Label_{tgt:04X};"));
        }
        "beq" | "beq.s" => cmp_branch(out, stack, ins, "=="),
        "bge" | "bge.s" => cmp_branch(out, stack, ins, ">="),
        "bgt" | "bgt.s" => cmp_branch(out, stack, ins, ">"),
        "ble" | "ble.s" => cmp_branch(out, stack, ins, "<="),
        "blt" | "blt.s" => cmp_branch(out, stack, ins, "<"),
        "bne.un" | "bne.un.s" => cmp_branch(out, stack, ins, "!="),
        "bge.un" | "bge.un.s" => cmp_branch(out, stack, ins, ">="),
        "bgt.un" | "bgt.un.s" => cmp_branch(out, stack, ins, ">"),
        "ble.un" | "ble.un.s" => cmp_branch(out, stack, ins, "<="),
        "blt.un" | "blt.un.s" => cmp_branch(out, stack, ins, "<"),

        "ceq" => cmp_op(stack, "=="),
        "cgt" => cmp_op(stack, ">"),
        "cgt.un" => cmp_op(stack, "!="),
        "clt" => cmp_op(stack, "<"),
        "clt.un" => cmp_op(stack, "<"),

        "switch" => {
            let v = pop(stack);
            if let Operand::Switch(ts) = &ins.operand {
                let base = ins.offset + ins.size;
                out.push(format!("        switch ({v})"));
                out.push("        {".into());
                for (i, t) in ts.iter().enumerate() {
                    let tgt = target_offset_from(base, *t);
                    out.push(format!("            case {i}: goto Label_{tgt:04X};"));
                }
                out.push("        }".into());
            }
        }

        "initobj" => {
            if let Operand::Token(tok) = &ins.operand {
                let t = type_token_name(reader, *tok);
                let addr = pop(stack);
                stmt(out, format!("{addr} = default({t});"));
            }
        }
        "ldtoken" => {
            if let Operand::Token(tok) = &ins.operand {
                let t = type_token_name(reader, *tok);
                push(stack, format!("typeof({t})"));
            }
        }
        "sizeof" => {
            if let Operand::Token(tok) = &ins.operand {
                let t = type_token_name(reader, *tok);
                push(stack, format!("sizeof({t})"));
            }
        }
        "ldftn" => {
            if let Operand::Token(tok) = &ins.operand {
                let (t, m) = method_ref_name(reader, *tok);
                push(stack, format!("{t}.{m}"));
            }
        }

        // Prefixes: ignore (they apply to the next instruction).
        "constrained." | "volatile." | "tail." | "readonly." | "unaligned." => {}

        "endfinally" | "endfilter" => stmt(out, "// end finally".into()),

        _ => return Ok(false),
    }
    Ok(true)
}

fn var_index(op: &Operand) -> u32 {
    match op {
        Operand::ShortVar(v) => *v as u32,
        Operand::Var(v) => *v as u32,
        _ => 0,
    }
}

fn local_name(local_names: &[String], idx: usize) -> String {
    local_names.get(idx).cloned().unwrap_or_else(|| format!("V_{idx}"))
}

fn store_local(out: &mut Vec<String>, stack: &mut Vec<String>, local_names: &[String], idx: usize) {
    let v = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let lname = local_name(local_names, idx);
    out.push(format!("        {lname} = {v};"));
}

/// Precedence level of a binary operator (higher = binds tighter).
/// Returns 0 for unknown operators.
fn prec(op: &str) -> u8 {
    match op {
        "*" | "/" | "%" => 7,
        "+" | "-" => 6,
        "<<" | ">>" => 5,
        "<" | "<=" | ">" | ">=" | "==" | "!=" => 4,
        "&" => 3,
        "^" => 2,
        "|" => 1,
        "&&" | "||" => 0,
        _ => 0,
    }
}

/// Strip outer parentheses from an expression if present.
fn strip_outer_parens(s: &str) -> &str {
    let t = s.trim();
    if t.starts_with('(') && t.ends_with(')') {
        // Check the parens are balanced and match.
        let inner = &t[1..t.len()-1];
        let mut depth = 0;
        for (i, c) in inner.chars().enumerate() {
            match c {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        // Unbalanced — these parens don't match.
                        return t;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            let _ = i;
        }
        if depth == 0 {
            return inner.trim();
        }
    }
    t
}

/// Get the top-level operator of an expression (for precedence comparison).
/// Returns None for atoms (variables, literals, calls).
fn top_op(s: &str) -> Option<&str> {
    let t = strip_outer_parens(s);
    // If the stripped expression still has parens at top level, it's an atom.
    // Find the lowest-precedence operator at depth 0.
    let mut depth = 0;
    let mut best: Option<(&str, usize)> = None;
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ if depth == 0 => {
                // Check for two-char operators.
                let two = if i + 1 < bytes.len() {
                    std::str::from_utf8(&bytes[i..i+2]).ok()
                } else { None };
                if let Some(two) = two {
                    if matches!(two, "<<" | ">>" | "<=" | ">=" | "==" | "!=" | "&&" | "||") {
                        let p = prec(two) as usize;
                        if best.map(|(_, bp)| p <= bp).unwrap_or(true) {
                            best = Some((two, p));
                        }
                        i += 2;
                        continue;
                    }
                }
                let one = std::str::from_utf8(&bytes[i..i+1]).ok();
                if let Some(one) = one {
                    if matches!(one, "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "<" | ">") {
                        let p = prec(one) as usize;
                        if best.map(|(_, bp)| p <= bp).unwrap_or(true) {
                            best = Some((one, p));
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    best.map(|(op, _)| op)
}

fn binop(stack: &mut Vec<String>, op: &str) {
    let b = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let a = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let my_prec = prec(op);
    // Strip outer parens from operands if their top-level op has >= precedence.
    let a_stripped = if top_op(&a).map(|o| prec(o) >= my_prec).unwrap_or(true) {
        strip_outer_parens(&a)
    } else {
        a.as_str()
    };
    let b_stripped = if top_op(&b).map(|o| prec(o) > my_prec).unwrap_or(true) {
        strip_outer_parens(&b)
    } else {
        b.as_str()
    };
    stack.push(format!("{a_stripped} {op} {b_stripped}"));
}

fn unop(stack: &mut Vec<String>, op: &str) {
    let a = stack.pop().unwrap_or_else(|| "/*?*/".into());
    stack.push(format!("{op}{a}"));
}

fn conv(stack: &mut Vec<String>, ty: &str) {
    let a = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let a_stripped = strip_outer_parens(&a);
    stack.push(format!("({ty})({a_stripped})"));
}

fn cmp_op(stack: &mut Vec<String>, op: &str) {
    let b = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let a = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let a_s = strip_outer_parens(&a);
    let b_s = strip_outer_parens(&b);
    stack.push(format!("({a_s} {op} {b_s} ? 1 : 0)"));
}

fn cmp_branch(out: &mut Vec<String>, stack: &mut Vec<String>, ins: &Instruction, op: &str) {
    let tgt = branch_target_of(ins);
    let b = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let a = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let a_s = strip_outer_parens(&a);
    let b_s = strip_outer_parens(&b);
    out.push(format!("        if ({a_s} {op} {b_s}) goto Label_{tgt:04X};"));
}

fn branch_target_of(ins: &Instruction) -> usize {
    match &ins.operand {
        Operand::BrTarget(o) => target_offset(ins.offset, ins.size, *o as i32),
        Operand::ShortBrTarget(o) => target_offset(ins.offset, ins.size, *o as i32),
        _ => ins.offset,
    }
}

fn build_newobj(reader: &Reader<'_>, tok: u32, stack: &mut Vec<String>) -> String {
    let (owner, _mname, sig) = match resolve_method(reader, tok) {
        Some(v) => v,
        None => return "new /*?*/()".into(),
    };
    let argc = sig.param_types.len();
    let mut args: Vec<String> = Vec::with_capacity(argc);
    for _ in 0..argc {
        args.push(stack.pop().unwrap_or_else(|| "/*?*/".into()));
    }
    args.reverse();
    format!("new {owner}({})", args.join(", "))
}

/// Resolve a method token to (owner type name, method name, signature).
fn resolve_method(reader: &Reader<'_>, tok: u32) -> Option<(String, String, MethodSig)> {
    let table = (tok >> 24) as u8;
    let row = (tok & 0x00FF_FFFF) as usize;
    match table {
        tbl::METHODDEF => {
            let r = reader.tables.get(tbl::METHODDEF).get(row - 1)?;
            let name = reader.method_name(r);
            let sig = reader.method_sig(r).ok()?;
            // Owner type: find the TypeDef whose method range contains this row.
            let owner = method_owner_type(reader, row as u32).unwrap_or_default();
            Some((owner, name, sig))
        }
        tbl::MEMBERREF => {
            let r = reader.tables.get(tbl::MEMBERREF).get(row - 1)?;
            let name = reader.member_ref_name(r);
            let sig = reader.member_ref_sig(r).ok()?;
            let parent = reader.member_ref_parent(r);
            let owner = reader.type_def_or_ref_name(parent);
            Some((strip_system(&owner), name, sig))
        }
        _ => None,
    }
}

fn method_owner_type(reader: &Reader<'_>, method_row: u32) -> Option<String> {
    for (i, t) in reader.tables.get(tbl::TYPEDEF).iter().enumerate() {
        let start = t.col(5);
        let next = reader
            .tables
            .get(tbl::TYPEDEF)
            .get(i + 1)
            .map(|r| r.col(5))
            .unwrap_or_else(|| reader.tables.row_count(tbl::METHODDEF) + 1);
        if method_row >= start && method_row < next {
            let ns = reader.type_def_namespace(t);
            let name = reader.type_def_name(t);
            return Some(strip_system(&if ns.is_empty() { name } else { format!("{ns}.{name}") }));
        }
    }
    None
}

fn method_ref_name(reader: &Reader<'_>, tok: u32) -> (String, String) {
    if let Some((owner, name, _)) = resolve_method(reader, tok) {
        (owner, name)
    } else {
        ("?".into(), "?".into())
    }
}

fn field_ref(reader: &Reader<'_>, tok: u32) -> (String, String) {
    let table = (tok >> 24) as u8;
    let row = (tok & 0x00FF_FFFF) as usize;
    match table {
        tbl::FIELD => {
            if let Some(r) = reader.tables.get(tbl::FIELD).get(row - 1) {
                let owner = field_owner_type(reader, row as u32).unwrap_or_default();
                return (owner, reader.field_name(r));
            }
        }
        tbl::MEMBERREF => {
            if let Some(r) = reader.tables.get(tbl::MEMBERREF).get(row - 1) {
                let parent = reader.member_ref_parent(r);
                let owner = strip_system(&reader.type_def_or_ref_name(parent));
                return (owner, reader.member_ref_name(r));
            }
        }
        _ => {}
    }
    ("?".into(), "?".into())
}

fn field_owner_type(reader: &Reader<'_>, field_row: u32) -> Option<String> {
    for (i, t) in reader.tables.get(tbl::TYPEDEF).iter().enumerate() {
        let start = t.col(4);
        let next = reader
            .tables
            .get(tbl::TYPEDEF)
            .get(i + 1)
            .map(|r| r.col(4))
            .unwrap_or_else(|| reader.tables.row_count(tbl::FIELD) + 1);
        if field_row >= start && field_row < next {
            let ns = reader.type_def_namespace(t);
            let name = reader.type_def_name(t);
            return Some(strip_system(&if ns.is_empty() { name } else { format!("{ns}.{name}") }));
        }
    }
    None
}

fn type_token_name(reader: &Reader<'_>, tok: u32) -> String {
    let table = (tok >> 24) as u8;
    let row = (tok & 0x00FF_FFFF) as usize;
    let ci = match table {
        tbl::TYPEDEF => CodedIndex { table: Some(tbl::TYPEDEF), row: row as u32 },
        tbl::TYPEREF => CodedIndex { table: Some(tbl::TYPEREF), row: row as u32 },
        tbl::TYPESPEC => CodedIndex { table: Some(tbl::TYPESPEC), row: row as u32 },
        _ => return "?".into(),
    };
    strip_system(&reader.type_def_or_ref_name(ci))
}

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

// ---- Flag decoding ----

fn type_access(flags: u32) -> &'static str {
    match flags & 0x00000007 {
        0 => "", // NotPublic (internal-ish)
        1 => "public",
        2 => "public",   // NestedPublic
        3 => "private",
        4 => "protected",
        5 => "internal",
        6 => "private protected",
        7 => "protected internal",
        _ => "",
    }
}

fn method_access(flags: u16) -> &'static str {
    match (flags & 0x0007) as u32 {
        0 => "",
        1 => "private",
        2 => "private protected",
        3 => "internal",
        4 => "protected",
        5 => "protected internal",
        6 => "public",
        _ => "",
    }
}

fn field_access(flags: u16) -> &'static str {
    match (flags & 0x0007) as u32 {
        0 => "private",
        1 => "private",
        2 => "private protected",
        3 => "internal",
        4 => "protected",
        5 => "protected internal",
        6 => "public",
        _ => "",
    }
}

/// Format a Constant table value blob (ECMA-335 II.23.1.16) as a C# literal.
/// `type_code` is the element type; `blob` is the raw little-endian bytes.
fn format_constant(type_code: u8, blob: &[u8]) -> String {
    match type_code {
        0x06 => i8::from_le_bytes([blob.get(0).copied().unwrap_or(0)]).to_string(),       // int8
        0x07 => u8::from_le_bytes([blob.get(0).copied().unwrap_or(0)]).to_string(),       // uint8
        0x08 => i16::from_le_bytes(blob[..2.min(blob.len())].try_into().unwrap_or([0, 0])).to_string(),  // int16
        0x09 => u16::from_le_bytes(blob[..2.min(blob.len())].try_into().unwrap_or([0, 0])).to_string(),  // uint16
        0x0a => i32::from_le_bytes(blob[..4.min(blob.len())].try_into().unwrap_or([0, 0, 0, 0])).to_string(),  // int32
        0x0b => u32::from_le_bytes(blob[..4.min(blob.len())].try_into().unwrap_or([0, 0, 0, 0])).to_string(),  // uint32
        0x0c => format!("{}L", i64::from_le_bytes(blob[..8.min(blob.len())].try_into().unwrap_or([0; 8])),),  // int64
        0x0d => format!("{}UL", u64::from_le_bytes(blob[..8.min(blob.len())].try_into().unwrap_or([0; 8])),), // uint64
        0x0e => format!("{}f", f32::from_le_bytes(blob[..4.min(blob.len())].try_into().unwrap_or([0, 0, 0, 0])),), // float32
        0x0f => f64::from_le_bytes(blob[..8.min(blob.len())].try_into().unwrap_or([0; 8])).to_string(), // float64
        _ => "0".into(),
    }
}

use crate::metadata::tables::decode_coded;
