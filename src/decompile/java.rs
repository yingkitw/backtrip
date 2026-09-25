//! Java source decompiler: class file → readable Java.
//!
//! Mirrors the C# back end's architecture: an expression-stack machine emits
//! a flat, goto-based statement list which is then folded into structured
//! source by the shared passes in [`super::flow`]. Java-specific notes:
//!
//! * javac 9+ compiles string concatenation to `invokedynamic
//!   makeConcatWithConstants` — the recipe from the BootstrapMethods
//!   attribute is decoded back into `+` expressions (JVMS 4.7.23).
//! * `long`/`double` locals occupy two slots; parameters are laid out the
//!   same way (JVMS 2.6.1).
//! * Branch offsets are relative to the opcode address, and the JVM pushes
//!   the thrown exception onto an *empty* stack at handler entry.
//! * Java has no `goto` — leftover jumps are commented out at the end.

use super::flow;
use super::DecompiledType;
use crate::error::Result;
use crate::java::classfile::{acc, Attribute, ClassFile, Code, Const, Member};
use crate::java::decoder::{decode, Instruction, Operand};
use std::collections::{HashMap, HashSet};

/// Semantic kind of a stack value — drives condition rendering
/// (`if (b)` vs `if (i == 0)`) and literal suffixes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Num,
    Bool,
    Str,
    Ref,
    Other,
}

#[derive(Clone)]
struct Val {
    expr: String,
    kind: Kind,
    /// Set for the placeholder pushed by `new`; completed by `<init>`.
    new_ref: Option<usize>,
    /// StringBuilder/StringBuffer accumulation marker (pre-9 class files).
    sb: bool,
}

impl Val {
    fn new(expr: impl Into<String>, kind: Kind) -> Val {
        Val { expr: expr.into(), kind, new_ref: None, sb: false }
    }
}

#[derive(Clone)]
struct LocalInfo {
    name: String,
    ty: Option<String>,
    declared: bool,
}

/// Decompile a whole class file into one Java source unit.
pub fn decompile_class_file(data: &[u8]) -> Result<DecompiledType> {
    let cf = ClassFile::parse(data)?;
    decompile_class(&cf)
}

/// Decompile every `.class` entry of a jar archive into Java source units.
/// Non-class entries (manifests, resources) are skipped.
pub fn decompile_jar(data: &[u8]) -> Result<Vec<DecompiledType>> {
    let entries = crate::java::jar::read_entries(data)?;
    let mut out = Vec::new();
    for e in entries {
        if !e.name.ends_with(".class") {
            continue;
        }
        match decompile_class_file(&e.data) {
            Ok(t) => out.push(t),
            // A jar can contain odd class entries (old versions, tools).
            // Skip the broken ones instead of failing the whole archive.
            Err(_) => continue,
        }
    }
    Ok(out)
}

/// The dotted class names of every `.class` entry in a jar (`--list`).
pub fn jar_class_names(data: &[u8]) -> Result<Vec<String>> {
    let entries = crate::java::jar::read_entries(data)?;
    Ok(entries
        .iter()
        .filter(|e| e.name.ends_with(".class"))
        .map(|e| crate::java::classfile::to_dotted(e.name.trim_end_matches(".class")))
        .collect())
}

pub fn decompile_class(cf: &ClassFile) -> Result<DecompiledType> {
    let source = render_class(cf)?;
    let simple = cf.simple_name();
    let pkg = cf.package();
    let file_name = if pkg.is_empty() {
        format!("{simple}.java")
    } else {
        format!("{}_{}.java", pkg.replace('.', "_"), simple)
    };
    Ok(DecompiledType { file_name, source })
}

/// The class's name for `--list` output.
pub fn list_name(data: &[u8]) -> Result<String> {
    let cf = ClassFile::parse(data)?;
    Ok(cf.this_name())
}

// ---- class rendering --------------------------------------------------------

fn render_class(cf: &ClassFile) -> Result<String> {
    let mut out = String::new();
    if let Some(src) = cf.source_file() {
        out.push_str(&format!("// source: {src}\n"));
        out.push_str(&format!(
            "// class file version {}.{}\n\n",
            cf.major,
            cf.minor
        ));
    }
    let pkg = cf.package();
    if !pkg.is_empty() {
        out.push_str(&format!("package {pkg};\n\n"));
    }

    let is_interface = cf.access_flags & acc::INTERFACE != 0;
    let is_annotation = cf.access_flags & acc::ANNOTATION != 0;
    let is_enum = cf.access_flags & acc::ENUM != 0;
    let simple = cf.simple_name();

    let mut mods = String::new();
    if cf.access_flags & acc::PUBLIC != 0 {
        mods.push_str("public ");
    }
    if !is_interface && !is_enum && cf.access_flags & acc::FINAL != 0 {
        mods.push_str("final ");
    }
    if cf.access_flags & acc::ABSTRACT != 0 && !is_interface && !is_annotation {
        mods.push_str("abstract ");
    }
    if cf.access_flags & acc::STRICT != 0 {
        mods.push_str("strictfp ");
    }

    let kind = if is_annotation {
        "@interface"
    } else if is_interface {
        "interface"
    } else if is_enum {
        "enum"
    } else {
        "class"
    };

    // Generic type parameters from the Signature attribute.
    let type_params = type_params_of(cf);
    let generics = if type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", type_params.join(", "))
    };

    out.push_str(&format!("{mods}{kind} {simple}{generics}"));

    let mut bases: Vec<String> = Vec::new();
    let mut iface_list = cf.interface_names();
    if is_enum {
        iface_list.clear(); // enums: javac lists Enum + interfaces; render interfaces only
    }
    if !is_interface {
        if let Some(sup) = cf.super_name() {
            // Implicit bases: Object for classes, Enum for enums.
            if sup != "java.lang.Record" && !(is_enum && sup == "java.lang.Enum") {
                bases.push(format!("extends {}", shorten(cf, &sup)));
            }
        }
    } else {
        // Interfaces extend other interfaces.
        if let Some(first) = iface_list.first().cloned() {
            iface_list.remove(0);
            let mut ext = vec![shorten(cf, &first)];
            ext.extend(iface_list.iter().map(|i| shorten(cf, i)));
            bases.push(format!("extends {}", ext.join(", ")));
            iface_list.clear();
        }
    }
    if !iface_list.is_empty() {
        let impls: Vec<String> = iface_list.iter().map(|i| shorten(cf, i)).collect();
        bases.push(format!("implements {}", impls.join(", ")));
    }
    if !bases.is_empty() {
        out.push_str(&format!(" {}", bases.join(" ")));
    }
    out.push_str(" {\n");

    // ---- enum constants ----
    let enum_consts: Vec<&Member> = if is_enum {
        cf.fields
            .iter()
            .filter(|f| f.access_flags & acc::ENUM != 0)
            .collect()
    } else {
        Vec::new()
    };
    if !enum_consts.is_empty() {
        let names: Vec<String> = enum_consts.iter().map(|f| f.name.clone()).collect();
        out.push_str(&format!("    {};\n\n", names.join(", ")));
    }

    // ---- fields ----
    for f in &cf.fields {
        if f.access_flags & acc::ENUM != 0 {
            continue;
        }
        if f.access_flags & acc::SYNTHETIC != 0 || f.name.starts_with('$') {
            continue;
        }
        if let Some(line) = render_field(cf, f) {
            out.push_str(&line);
        }
    }
    if cf.fields.iter().any(|f| render_field(cf, f).is_some()) {
        out.push('\n');
    }

    // ---- methods ----
    for m in &cf.methods {
        if is_enum && is_enum_utility(m) {
            continue;
        }
        if m.access_flags & acc::SYNTHETIC != 0 || m.access_flags & acc::BRIDGE != 0 {
            continue;
        }
        if let Some(block) = render_method(cf, m, is_enum)? {
            out.push_str(&block);
            out.push('\n');
        }
    }

    while out.ends_with("\n\n") {
        out.pop();
    }
    out.push_str("}\n");
    Ok(out)
}

fn type_params_of(cf: &ClassFile) -> Vec<String> {
    cf.attributes
        .iter()
        .find_map(|a| match a {
            Attribute::Signature(s) => Some(crate::java::descriptor::parse_class_type_params(s)),
            _ => None,
        })
        .unwrap_or_default()
}

fn is_enum_utility(m: &Member) -> bool {
    // javac's generated members: values(), valueOf(String), $VALUES, and
    // the (String, int) constructor backing the constants.
    (m.name == "values" && m.descriptor.starts_with("()[L"))
        || (m.name == "valueOf" && m.descriptor.contains("(Ljava/lang/String;)L"))
        || m.name == "$values"
        || (m.name == "<init>" && m.descriptor.starts_with("(Ljava/lang/String;I)"))
}

fn render_field(cf: &ClassFile, f: &Member) -> Option<String> {
    // Prefer the generic Signature attribute; fall back to the erased descriptor.
    let sig = f.attributes.iter().find_map(|a| match a {
        Attribute::Signature(s) => Some(s.clone()),
        _ => None,
    });
    let raw = match sig {
        Some(s) => crate::java::descriptor::parse_type_signature(&s).ok()?,
        None => crate::java::descriptor::parse_field_descriptor(&f.descriptor).ok()?,
    };
    let ty = shorten_type(cf, &raw);
    let mods = member_mods(f.access_flags, true);
    let mut init = String::new();
    if let Some(c) = f.constant_value() {
        init = format!(" = {}", const_literal(&c));
    }
    Some(format!("    {mods}{ty} {}{init};\n", f.name))
}

/// Shorten `java.lang.*`/same-package prefixes inside a rendered *type*,
/// including names nested inside generic arguments
/// (`Map<java.lang.String, java.util.List<T>>[]` →
/// `Map<String, List<T>>[]`). Works by scanning maximal dotted-name runs.
fn shorten_type(cf: &ClassFile, ty: &str) -> String {
    let mut out = String::with_capacity(ty.len());
    let mut run = String::new();
    let flush = |run: &str, out: &mut String| {
        if run.contains('.') {
            out.push_str(&shorten(cf, run));
        } else {
            out.push_str(run);
        }
    };
    for c in ty.chars() {
        if c.is_alphanumeric() || c == '_' || c == '$' || c == '.' {
            run.push(c);
        } else {
            if !run.is_empty() {
                flush(&run, &mut out);
                run.clear();
            }
            out.push(c);
        }
    }
    if !run.is_empty() {
        flush(&run, &mut out);
    }
    out
}

/// Render one method: returns `None` when the method should be skipped.
fn render_method(cf: &ClassFile, m: &Member, is_enum: bool) -> Result<Option<String>> {
    let is_ctor = m.name == "<init>";
    if m.name == "<clinit>" {
        if is_enum {
            return Ok(None); // enum bookkeeping ($VALUES setup)
        }
        // Render the static initializer block.
        if let Some(code) = m.code() {
            let body = decompile_body(cf, code, m)?;
            let mut block = String::from("    static {\n");
            for line in body {
                block.push_str(&line);
                block.push('\n');
            }
            block.push_str("    }\n");
            return Ok(Some(block));
        }
        return Ok(None);
    }

    // Prefer the generic Signature attribute (JVMS 4.7.9.1) — renders
    // `List<String>`, type variables, wildcards; fall back to the erased
    // descriptor.
    let sig_attr = m.attributes.iter().find_map(|a| match a {
        Attribute::Signature(s) => Some(s.clone()),
        _ => None,
    });
    let (method_type_params, params, ret) = match &sig_attr {
        Some(sig) => match crate::java::descriptor::parse_method_signature(sig) {
            Ok((tps, ps, ret)) => (tps, ps, ret),
            Err(_) => {
                let (ps, ret) =
                    crate::java::descriptor::parse_method_descriptor(&m.descriptor)?;
                (Vec::new(), ps, ret)
            }
        },
        None => {
            let (ps, ret) = crate::java::descriptor::parse_method_descriptor(&m.descriptor)?;
            (Vec::new(), ps, ret)
        }
    };
    let params: Vec<String> = params.iter().map(|p| shorten_type(cf, p)).collect();
    let ret = shorten_type(cf, &ret);
    let is_static = m.access_flags & acc::STATIC != 0;
    let name = if is_ctor { cf.simple_name() } else { m.name.clone() };

    let mut mods = member_mods(m.access_flags, false);
    if is_ctor {
        // Constructors cannot be static/final/abstract/synchronized-declared.
        mods = mods
            .replace("final ", "")
            .replace("abstract ", "")
            .replace("synchronized ", "");
    }
    if !method_type_params.is_empty() && !is_ctor {
        mods = format!("{mods}<{}> ", method_type_params.join(", "));
    }

    // Parameter names: prefer LocalVariableTable, fall back to argN.
    let names = param_names(m, &params, is_static);
    let params_str = names
        .iter()
        .zip(&params)
        .map(|(n, t)| format!("{t} {n}"))
        .collect::<Vec<_>>()
        .join(", ");

    let throws = m.throws();
    let throws_str = if throws.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = throws.iter().map(|t| shorten(cf, &to_dotted(t))).collect();
        format!(" throws {}", list.join(", "))
    };

    let header = if is_ctor {
        format!("    {mods}{name}({params_str}){throws_str}")
    } else {
        format!("    {mods}{ret} {name}({params_str}){throws_str}")
    };

    let code = m.code();
    if code.is_none() || m.access_flags & acc::ABSTRACT != 0 || m.access_flags & acc::NATIVE != 0 {
        return Ok(Some(format!("{header};\n")));
    }

    let body = decompile_body(cf, code.unwrap(), m)?;
    let mut out = format!("{header} {{\n");
    for line in body {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("    }\n");
    Ok(Some(out))
}

fn member_mods(flags: u16, is_field: bool) -> String {
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
    if is_field {
        if flags & acc::VOLATILE_BRIDGE != 0 {
            out.push("volatile");
        }
        if flags & acc::TRANSIENT_VARARGS != 0 {
            out.push("transient");
        }
    } else if flags & acc::SYNCHRONIZED_SUPER != 0 {
        out.push("synchronized");
    } else if flags & acc::ABSTRACT != 0 {
        out.push("abstract");
    }
    if flags & acc::NATIVE != 0 {
        out.push("native");
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{} ", out.join(" "))
    }
}

// ---- body decompilation -----------------------------------------------------

/// Decompile one method body into indented statement lines.
fn decompile_body(cf: &ClassFile, code: &Code, m: &Member) -> Result<Vec<String>> {
    let instrs = decode(&code.code)?;
    let is_static = m.access_flags & acc::STATIC != 0;
    let desc = &m.descriptor;

    // Local slot layout (JVMS 2.6.1): instance methods start with `this`;
    // long/double parameters occupy two slots.
    let mut locals: HashMap<u16, LocalInfo> = HashMap::new();
    let mut next_slot: u16 = 0;
    if !is_static {
        locals.insert(
            0,
            LocalInfo { name: "this".into(), ty: None, declared: true },
        );
        next_slot = 1;
    }
    let (params, _) = crate::java::descriptor::parse_method_descriptor(desc)?;
    for (next_param, p) in params.iter().enumerate() {
        let wide = *p == "long" || *p == "double";
        let name =
            lvt_name(code, next_slot).unwrap_or_else(|| format!("arg{next_param}"));
        locals.insert(
            next_slot,
            LocalInfo { name, ty: Some(p.clone()), declared: true },
        );
        next_slot += if wide { 2 } else { 1 };
    }
    // LVT-named non-parameter locals.
    for lv in &code.local_vars {
        if lv.index >= next_slot && !locals.contains_key(&lv.index) {
            let ty = crate::java::descriptor::parse_field_descriptor(&lv.descriptor).ok();
            locals.insert(
                lv.index,
                LocalInfo { name: lv.name.clone(), ty, declared: false },
            );
        }
    }

    // Exception handlers: the JVM pushes the throwable onto an empty stack
    // at handler entry; the first `astore` names the catch variable. Slots
    // are bound lazily during the walk — javac reuses the same slot for
    // method locals and catch variables in disjoint live ranges, so pre-
    // binding would corrupt the body code before the handler.
    let exception_slots: HashMap<u16, String> = HashMap::new();
    let handler_starts: HashSet<usize> = code
        .exception_table
        .iter()
        .map(|e| e.handler_pc as usize)
        .collect();
    // Handlers reached via a catch-all entry (finally): their rethrow is
    // compiler machinery, suppressed for valid Java.
    let finally_handlers: HashSet<usize> = code
        .exception_table
        .iter()
        .filter(|e| e.catch_type == 0)
        .map(|e| e.handler_pc as usize)
        .collect();

    // Synchronized-block machinery: a catch-all handler shaped
    // `astore; aload; monitorexit` is the monitor re-release + rethrow, not
    // user EH. Its ranges get no try/finally markers, and monitorexit inside
    // them emits nothing.
    let sync_handlers: HashSet<usize> = code
        .exception_table
        .iter()
        .map(|e| e.handler_pc as usize)
        .collect::<HashSet<_>>()
        .into_iter()
        .filter(|&h| is_sync_handler(&instrs, h))
        .collect();
    let mut sync_region_offsets: HashSet<usize> = HashSet::new();
    if !sync_handlers.is_empty() {
        let mut all_starts: Vec<usize> = handler_starts.iter().copied().collect();
        all_starts.sort();
        for &h in &sync_handlers {
            let end = all_starts
                .iter()
                .find(|&&s| s > h)
                .copied()
                .unwrap_or(code.code.len());
            for ins in instrs.iter().filter(|i| i.offset >= h && i.offset < end) {
                sync_region_offsets.insert(ins.offset);
            }
        }
    }

    // Pre-compute branch targets for label emission. Exception try-region
    // starts and handler entry points are targets too — the JVM transfers
    // control there implicitly, and we need label lines to anchor the
    // try/catch markers.
    let mut targets: HashSet<usize> = HashSet::new();
    for ins in &instrs {
        match &ins.operand {
            Operand::Target(t) => {
                targets.insert(*t);
            }
            Operand::TableSwitch { default, targets: ts, .. } => {
                targets.insert(*default);
                targets.extend(ts.iter());
            }
            Operand::LookupSwitch { default, pairs, .. } => {
                targets.insert(*default);
                targets.extend(pairs.iter().map(|(_, t)| t));
            }
            _ => {}
        }
    }
    for e in &code.exception_table {
        targets.insert(e.start_pc as usize);
        targets.insert(e.handler_pc as usize);
    }

    let mut stack: Vec<Val> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    let mut offset_to_line: HashMap<usize, usize> = HashMap::new();
    let mut state = State {
        cf,
        locals,
        exception_slots,
        finally_thrown: HashSet::new(),
        new_counter: 0,
        incoming: HashMap::new(),
        declared_temps: HashSet::new(),
        sync_exits: sync_region_offsets,
    };

    let mut i = 0usize;
    let mut handler_counter = 0usize;
    while i < instrs.len() {
        let ins = &instrs[i];

        // Anchor labels / join frames for this offset (all paths).
        emit_labels(&mut state, &mut stack, &mut out, &mut offset_to_line, ins.offset, &targets);

        // Handler entry: name the thrown exception and skip the store —
        // the catch/finally clause declares the variable.
        if stack.is_empty()
            && handler_starts.contains(&ins.offset)
            && (ins.op == 0x3A || (0x4B..=0x4E).contains(&ins.op))
        {
            let slot = if ins.op == 0x3A {
                match ins.operand {
                    Operand::Local(s) => Some(s),
                    _ => None,
                }
            } else {
                Some((ins.op - 0x4B) as u16)
            };
            if let Some(slot) = slot {
                if let std::collections::hash_map::Entry::Vacant(e) = state.exception_slots.entry(slot) {
                    let name = if handler_counter == 0 {
                        "e".to_string()
                    } else {
                        format!("e{}", handler_counter + 1)
                    };
                    e.insert(name.clone());
                    if finally_handlers.contains(&ins.offset) {
                        state.finally_thrown.insert(name.clone());
                    }
                    state.locals.insert(
                        slot,
                        LocalInfo { name, ty: None, declared: true },
                    );
                    handler_counter += 1;
                }
                i += 1;
                continue;
            }
        }

        // Monitor stash: `dup; astore N; monitorenter` — javac stashes the
        // monitor in a temp local; suppress the store (the other dup copy
        // feeds monitorenter directly).
        if (ins.op == 0x3A || (0x4B..=0x4E).contains(&ins.op))
            && i > 0
            && instrs[i - 1].op == 0x59
            && instrs.get(i + 1).map(|n| n.op) == Some(0xC2)
        {
            stack.pop();
            i += 1;
            continue;
        }

        // Combined compare + branch (lcmp/fcmp/dcmp followed by if*).
        if is_compare_op(ins.name)
            && let Some(next) = instrs.get(i + 1)
                && is_compare_branch(next.name) {
                    compare_branch(&mut state, &mut stack, &mut out, next);
                    i += 2;
                    continue;
                }

        offset_to_line.insert(ins.offset, out.len());
        if !handle_instr(&mut state, ins, &mut stack, &mut out)? {
            out.push(format!("        // unsupported: {}", ins.name));
            stack.clear();
        }
        i += 1;
    }

    // Structure the flat statement list.
    insert_exception_markers(&mut out, &offset_to_line, code, cf, &sync_handlers);
    // Label comments right after a clause header are anchors only — drop them.
    let mut i = 0;
    while i < out.len() {
        let prev_is_header = i > 0 && {
            let t = out[i - 1].trim();
            t == "try {" || t == "finally {" || t.starts_with("catch (")
        };
        if prev_is_header && out[i].trim().starts_with("// Label_") {
            out.remove(i);
            continue;
        }
        i += 1;
    }
    flow::restructure_while_loops_javac(&mut out);
    flow::restructure_if_else(&mut out);
    flow::restructure_do_while_loops(&mut out);
    flow::restructure_while_loops(&mut out);
    // Compact pass-blanked lines so the foreach pass sees adjacent lines.
    flow::drop_empty_lines(&mut out);
    restructure_foreach(&mut out);
    flow::restructure_for_loops(&mut out);
    flow::restructure_switch(&mut out);
    restructure_boolean_returns(&mut out);
    restructure_ternary(&mut out);
    flow::remove_unreferenced_labels(&mut out);
    flow::drop_empty_lines(&mut out);
    flow::remove_redundant_jump_pairs(&mut out);
    flow::cleanup_goto_labels(&mut out);
    flow::drop_empty_lines(&mut out);
    // javac emits an explicit `return` at the end of every void method and
    // constructor; source style omits it.
    if out.last().map(|l| l.trim() == "return;").unwrap_or(false) {
        out.pop();
    }
    Ok(out)
}

struct State<'a> {
    cf: &'a ClassFile,
    locals: HashMap<u16, LocalInfo>,
    exception_slots: HashMap<u16, String>,
    /// Names of catch-all (finally) handler variables — their compiler-
    /// generated rethrow (`athrow`) is suppressed for valid Java.
    finally_thrown: HashSet<String>,
    new_counter: usize,
    /// Branch-join frames: bytecode offset → temp variable names carrying
    /// stack values across basic-block boundaries. The JVM verifier
    /// guarantees a consistent stack depth per join point, so a fixed set
    /// of temps per target is sound.
    incoming: HashMap<usize, Vec<String>>,
    /// Join temps already declared (`var`) — later assignments are plain.
    declared_temps: HashSet<String>,
    /// Offsets of `monitorexit` instructions inside synchronized-block
    /// handler regions — the exceptional-path release emits nothing.
    sync_exits: HashSet<usize>,
}

/// Get (or create) the temp names carrying values into a join point.
fn ensure_incoming(
    incoming: &mut HashMap<usize, Vec<String>>,
    target: usize,
    n: usize,
) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    match incoming.get(&target) {
        Some(v) if v.len() == n => v.clone(),
        _ => {
            let v: Vec<String> =
                (0..n).map(|i| format!("T{:04X}_{i}", target)).collect();
            incoming.insert(target, v.clone());
            v
        }
    }
}

/// Emit assignments carrying `vals` into `target`'s frame; returns the temp
/// names (== `vals` when they are already the frame temps). First textual
/// assignment declares the temp with `var`.
fn flush_to(
    state: &mut State,
    out: &mut Vec<String>,
    vals: &[Val],
    target: usize,
) -> Vec<String> {
    if vals.is_empty() {
        return Vec::new();
    }
    let names = ensure_incoming(&mut state.incoming, target, vals.len());
    let identical = vals.iter().zip(&names).all(|(v, n)| v.expr == *n);
    if !identical {
        for (n, v) in names.iter().zip(vals) {
            if state.declared_temps.contains(n) {
                out.push(format!("        {n} = {};", v.expr));
            } else {
                out.push(format!("        var {n} = {};", v.expr));
                state.declared_temps.insert(n.clone());
            }
        }
    }
    names
}

/// Replace the stack with a join frame's temps (fall-through continues with
/// the same values the taken branch will see).
fn transfer_frame(stack: &mut Vec<Val>, names: Vec<String>) {
    stack.clear();
    for n in names {
        stack.push(Val::new(n, Kind::Other));
    }
}

fn emit_labels(
    state: &mut State,
    stack: &mut Vec<Val>,
    out: &mut Vec<String>,
    offset_to_line: &mut HashMap<usize, usize>,
    offset: usize,
    targets: &HashSet<usize>,
) {
    if !targets.contains(&offset) {
        return;
    }
    // Fall-through values reaching this join are flushed into the frame
    // temps *before* the label so both predecessors assign the same names.
    if !stack.is_empty() {
        let names = flush_to(state, out, stack, offset);
        let identical = stack.iter().zip(&names).all(|(v, n)| v.expr == *n);
        if !identical {
            transfer_frame(stack, names);
        }
    } else if let Some(names) = state.incoming.get(&offset).cloned() {
        // Reached after a control transfer flushed this frame: restore it.
        transfer_frame(stack, names);
    }
    offset_to_line.insert(offset, out.len());
    out.push(format!("        {}:", flow::label_name(offset)));
}

fn is_compare_op(name: &str) -> bool {
    matches!(name, "lcmp" | "fcmpl" | "fcmpg" | "dcmpl" | "dcmpg")
}

fn is_compare_branch(name: &str) -> bool {
    matches!(
        name,
        "ifeq" | "ifne" | "iflt" | "ifge" | "ifgt" | "ifle"
    )
}

/// Emit `if (a OP b) goto Label;` for a compare+branch pair, flushing any
/// values below the operands so both paths see them.
fn compare_branch(state: &mut State, stack: &mut Vec<Val>, out: &mut Vec<String>, branch: &Instruction) {
    let b = pop(stack);
    let a = pop(stack);
    let op = match branch.name {
        "ifeq" => "==",
        "ifne" => "!=",
        "iflt" => "<",
        "ifge" => ">=",
        "ifgt" => ">",
        _ => "<=",
    };
    let cond = flow::paren_if_needed(&a.expr, op);
    let cond_b = flow::paren_if_needed(&b.expr, op);
    let target = match branch.operand {
        Operand::Target(t) => t,
        _ => 0,
    };
    if !stack.is_empty() {
        let names = flush_to(state, out, stack, target);
        transfer_frame(stack, names);
    }
    out.push(format!(
        "        if ({cond} {op} {cond_b}) goto {};",
        flow::label_name(target)
    ));
}

fn pop(stack: &mut Vec<Val>) -> Val {
    stack.pop().unwrap_or_else(|| Val::new("/*?*/", Kind::Other))
}

/// Pop `n` arguments (they sit on the stack in reverse order).
fn pop_args(stack: &mut Vec<Val>, n: usize) -> Vec<String> {
    let mut vals = Vec::with_capacity(n);
    for _ in 0..n {
        vals.push(pop(stack));
    }
    vals.reverse();
    vals.into_iter().map(|v| v.expr).collect()
}

fn handle_instr(
    state: &mut State,
    ins: &Instruction,
    stack: &mut Vec<Val>,
    out: &mut Vec<String>,
) -> Result<bool> {
    let name = ins.name;
    let stmt = |out: &mut Vec<String>, s: String| out.push(format!("        {s}"));

    match name {
        "nop" => {}
        "aconst_null" => stack.push(Val::new("null", Kind::Ref)),
        "iconst_m1" => stack.push(Val::new("-1", Kind::Num)),
        "iconst_0" => stack.push(Val::new("0", Kind::Num)),
        "iconst_1" => stack.push(Val::new("1", Kind::Num)),
        "iconst_2" => stack.push(Val::new("2", Kind::Num)),
        "iconst_3" => stack.push(Val::new("3", Kind::Num)),
        "iconst_4" => stack.push(Val::new("4", Kind::Num)),
        "iconst_5" => stack.push(Val::new("5", Kind::Num)),
        "lconst_0" => stack.push(Val::new("0L", Kind::Num)),
        "lconst_1" => stack.push(Val::new("1L", Kind::Num)),
        "fconst_0" => stack.push(Val::new("0.0f", Kind::Num)),
        "fconst_1" => stack.push(Val::new("1.0f", Kind::Num)),
        "fconst_2" => stack.push(Val::new("2.0f", Kind::Num)),
        "dconst_0" => stack.push(Val::new("0.0", Kind::Num)),
        "dconst_1" => stack.push(Val::new("1.0", Kind::Num)),
        "bipush" | "sipush" => {
            let v = match ins.operand {
                Operand::I8(v) => v.to_string(),
                Operand::I16(v) => v.to_string(),
                _ => "0".into(),
            };
            stack.push(Val::new(v, Kind::Num));
        }
        "ldc" | "ldc_w" | "ldc2_w" => {
            let idx = match ins.operand {
                Operand::Cp(i) => i,
                _ => 0,
            };
            stack.push(ldc_val(state.cf, idx));
        }

        // ---- loads ----
        "iload" | "lload" | "fload" | "dload" | "aload" => {
            let slot = match ins.operand {
                Operand::Local(n) => n,
                _ => 0,
            };
            stack.push(load_local(&state.locals, slot, name));
        }
        "iload_0" | "iload_1" | "iload_2" | "iload_3" => {
            stack.push(load_local(&state.locals, (ins.op - 0x1A) as u16, "iload"));
        }
        "lload_0" | "lload_1" | "lload_2" | "lload_3" => {
            stack.push(load_local(&state.locals, (ins.op - 0x1E) as u16, "lload"));
        }
        "fload_0" | "fload_1" | "fload_2" | "fload_3" => {
            stack.push(load_local(&state.locals, (ins.op - 0x22) as u16, "fload"));
        }
        "dload_0" | "dload_1" | "dload_2" | "dload_3" => {
            stack.push(load_local(&state.locals, (ins.op - 0x26) as u16, "dload"));
        }
        "aload_0" | "aload_1" | "aload_2" | "aload_3" => {
            stack.push(load_local(&state.locals, (ins.op - 0x2A) as u16, "aload"));
        }

        // ---- array loads ----
        "iaload" | "laload" | "faload" | "daload" | "aaload" | "baload" | "caload" | "saload" => {
            let idx = pop(stack);
            let arr = pop(stack);
            let kind = if name == "aaload" { Kind::Ref } else { Kind::Num };
            stack.push(Val::new(format!("{}[{}]", arr.expr, idx.expr), kind));
        }

        // ---- stores ----
        "istore" | "lstore" | "fstore" | "dstore" | "astore" => {
            let slot = match ins.operand {
                Operand::Local(n) => n,
                _ => 0,
            };
            store_local(state, stack, out, slot);
        }
        "istore_0" | "istore_1" | "istore_2" | "istore_3" => {
            store_local(state, stack, out, (ins.op - 0x3B) as u16);
        }
        "lstore_0" | "lstore_1" | "lstore_2" | "lstore_3" => {
            store_local(state, stack, out, (ins.op - 0x3F) as u16);
        }
        "fstore_0" | "fstore_1" | "fstore_2" | "fstore_3" => {
            store_local(state, stack, out, (ins.op - 0x43) as u16);
        }
        "dstore_0" | "dstore_1" | "dstore_2" | "dstore_3" => {
            store_local(state, stack, out, (ins.op - 0x47) as u16);
        }
        "astore_0" | "astore_1" | "astore_2" | "astore_3" => {
            store_local(state, stack, out, (ins.op - 0x4B) as u16);
        }

        // ---- array stores ----
        "iastore" | "lastore" | "fastore" | "dastore" | "aastore" | "bastore" | "castore"
        | "sastore" => {
            let v = pop(stack);
            let idx = pop(stack);
            let arr = pop(stack);
            stmt(out, format!("{}[{}] = {};", arr.expr, idx.expr, v.expr));
        }

        // ---- stack ops ----
        "pop" | "pop2" => {
            let v = pop(stack);
            // A discarded non-trivial expression is still a side effect
            // (e.g. `list.add(x)` returns boolean and is popped).
            if is_side_effect(&v.expr) {
                stmt(out, format!("{};", v.expr));
            }
        }
        "dup" | "dup2" => {
            let n = if name == "dup" { 1 } else { 2 };
            let start = stack.len().saturating_sub(n);
            let tops: Vec<Val> = stack[start..].to_vec();
            stack.extend(tops);
        }
        "dup_x1" => {
            let v1 = pop(stack);
            let v2 = pop(stack);
            stack.push(v1.clone());
            stack.push(v2);
            stack.push(v1);
        }
        "dup_x2" => {
            let v1 = pop(stack);
            let v2 = pop(stack);
            let v3 = pop(stack);
            stack.push(v1.clone());
            stack.push(v3);
            stack.push(v2);
            stack.push(v1);
        }
        "dup2_x1" => {
            let v1 = pop(stack);
            let v2 = pop(stack);
            let v3 = pop(stack);
            stack.push(v2.clone());
            stack.push(v1.clone());
            stack.push(v3);
            stack.push(v2);
            stack.push(v1);
        }
        "dup2_x2" => {
            let v1 = pop(stack);
            let v2 = pop(stack);
            let v3 = pop(stack);
            let v4 = pop(stack);
            stack.push(v2.clone());
            stack.push(v1.clone());
            stack.push(v4);
            stack.push(v3);
            stack.push(v2);
            stack.push(v1);
        }
        "swap" => {
            let v1 = pop(stack);
            let v2 = pop(stack);
            stack.push(v1);
            stack.push(v2);
        }

        // ---- arithmetic ----
        "iadd" | "ladd" | "fadd" | "dadd" => binop_val(stack, "+"),
        "isub" | "lsub" | "fsub" | "dsub" => binop_val(stack, "-"),
        "imul" | "lmul" | "fmul" | "dmul" => binop_val(stack, "*"),
        "idiv" | "ldiv" | "fdiv" | "ddiv" => binop_val(stack, "/"),
        "irem" | "lrem" | "frem" | "drem" => binop_val(stack, "%"),
        "ishl" | "lshl" => binop_val(stack, "<<"),
        "ishr" | "lshr" => binop_val(stack, ">>"),
        "iushr" | "lushr" => binop_val(stack, ">>>"),
        "iand" | "land" => binop_val(stack, "&"),
        "ior" | "lor" => binop_val(stack, "|"),
        "ixor" | "lxor" => binop_val(stack, "^"),
        "ineg" | "lneg" | "fneg" | "dneg" => {
            let a = pop(stack);
            stack.push(Val::new(format!("-{}", a.expr), Kind::Num));
        }
        "iinc" => {
            let (slot, delta) = match ins.operand {
                Operand::Iinc(n, d) => (n as u16, d as i64),
                Operand::WideIinc(n, d) => (n, d as i64),
                _ => (0, 0),
            };
            ensure_declared(state, out, slot, "0");
            let lname = local_ref(&state.locals, slot);
            if delta >= 0 {
                stmt(out, format!("{lname} += {delta};"));
            } else {
                stmt(out, format!("{lname} -= {};", -delta));
            }
        }

        // ---- conversions ----
        "i2l" => conv_val(stack, "long"),
        "i2f" => conv_val(stack, "float"),
        "i2d" => conv_val(stack, "double"),
        "l2i" => conv_val(stack, "int"),
        "l2f" => conv_val(stack, "float"),
        "l2d" => conv_val(stack, "double"),
        "f2i" => conv_val(stack, "int"),
        "f2l" => conv_val(stack, "long"),
        "f2d" => conv_val(stack, "double"),
        "d2i" => conv_val(stack, "int"),
        "d2l" => conv_val(stack, "long"),
        "d2f" => conv_val(stack, "float"),
        "i2b" => conv_val(stack, "byte"),
        "i2c" => conv_val(stack, "char"),
        "i2s" => conv_val(stack, "short"),

        // ---- compare (standalone fallback; pairs handled earlier) ----
        "lcmp" | "fcmpl" | "fcmpg" | "dcmpl" | "dcmpg" => {
            let b = pop(stack);
            let a = pop(stack);
            stack.push(Val::new(
                format!("{} - {}", a.expr, b.expr),
                Kind::Num,
            ));
        }

        // ---- branches ----
        "ifeq" | "ifne" | "iflt" | "ifge" | "ifgt" | "ifle" => {
            let v = pop(stack);
            let cond = match (name, v.kind) {
                ("ifeq", Kind::Bool) => format!("!({})", v.expr),
                ("ifne", Kind::Bool) => v.expr.clone(),
                ("ifeq", Kind::Ref) => format!("{} == null", v.expr),
                ("ifne", Kind::Ref) => format!("{} != null", v.expr),
                ("ifeq", _) => format!("{} == 0", v.expr),
                ("ifne", _) => format!("{} != 0", v.expr),
                ("iflt", _) => format!("{} < 0", v.expr),
                ("ifge", _) => format!("{} >= 0", v.expr),
                ("ifgt", _) => format!("{} > 0", v.expr),
                _ => format!("{} <= 0", v.expr),
            };
            let target = branch_target(ins);
            if !stack.is_empty() {
                let names = flush_to(state, out, stack, target);
                transfer_frame(stack, names);
            }
            branch_stmt(out, &cond, target);
        }
        "if_icmpeq" | "if_icmpne" | "if_icmplt" | "if_icmpge" | "if_icmpgt" | "if_icmple"
        | "if_acmpeq" | "if_acmpne" => {
            let b = pop(stack);
            let a = pop(stack);
            let op = match name {
                "if_icmpeq" | "if_acmpeq" => "==",
                "if_icmpne" | "if_acmpne" => "!=",
                "if_icmplt" => "<",
                "if_icmpge" => ">=",
                "if_icmpgt" => ">",
                _ => "<=",
            };
            let cond = format!(
                "{} {op} {}",
                flow::paren_if_needed(&a.expr, op),
                flow::paren_if_needed(&b.expr, op)
            );
            let target = branch_target(ins);
            if !stack.is_empty() {
                let names = flush_to(state, out, stack, target);
                transfer_frame(stack, names);
            }
            branch_stmt(out, &cond, target);
        }
        "ifnull" => {
            let v = pop(stack);
            let target = branch_target(ins);
            if !stack.is_empty() {
                let names = flush_to(state, out, stack, target);
                transfer_frame(stack, names);
            }
            branch_stmt(out, &format!("{} == null", v.expr), target);
        }
        "ifnonnull" => {
            let v = pop(stack);
            let target = branch_target(ins);
            if !stack.is_empty() {
                let names = flush_to(state, out, stack, target);
                transfer_frame(stack, names);
            }
            branch_stmt(out, &format!("{} != null", v.expr), target);
        }
        "goto" | "goto_w" => {
            let target = branch_target(ins);
            if !stack.is_empty() {
                flush_to(state, out, stack, target);
                stack.clear();
            }
            out.push(format!("        goto {};", flow::label_name(target)));
        }
        "jsr" | "jsr_w" | "ret" => return Ok(false),

        // ---- switches ----
        "tableswitch" | "lookupswitch" => {
            let v = pop(stack);
            let (default, entries): (usize, Vec<(i64, usize)>) = match &ins.operand {
                Operand::TableSwitch { default, low, targets, .. } => (
                    *default,
                    targets
                        .iter()
                        .enumerate()
                        .map(|(k, t)| ((*low + k as i32) as i64, *t))
                        .collect(),
                ),
                Operand::LookupSwitch { default, pairs } => (
                    *default,
                    pairs.iter().map(|(v, t)| (*v as i64, *t)).collect(),
                ),
                _ => (0, Vec::new()),
            };
            out.push(format!("        switch ({})", v.expr));
            out.push("        {".into());
            let mut all_targets: Vec<usize> = entries.iter().map(|(_, t)| *t).collect();
            all_targets.push(default);
            all_targets.dedup();
            let leftover: Vec<Val> = stack.clone();
            stack.clear();
            for t in &all_targets {
                flush_to(state, out, &leftover, *t);
            }
            for (val, target) in &entries {
                out.push(format!(
                    "            case {val}: goto {};",
                    flow::label_name(*target)
                ));
            }
            out.push("        }".into());
            // Default target rendered as the trailing goto the shared
            // restructure_switch pass expects.
            out.push(format!("        goto {};", flow::label_name(default)));
        }

        // ---- returns ----
        "ireturn" | "lreturn" | "freturn" | "dreturn" | "areturn" => {
            let v = pop(stack);
            stmt(out, format!("return {};", v.expr));
        }
        "return" => {
            stmt(out, "return;".into());
        }

        // ---- fields ----
        "getstatic" => {
            let idx = cp_index(ins);
            let (class, field, desc) = state.cf.member_ref(idx);
            stack.push(Val::new(
                format!("{}.{}", shorten(state.cf, &to_dotted(&class)), field),
                kind_of_desc(&desc),
            ));
        }
        "putstatic" => {
            let idx = cp_index(ins);
            let v = pop(stack);
            let (class, field, _) = state.cf.member_ref(idx);
            stmt(out, format!("{}.{} = {};", shorten(state.cf, &to_dotted(&class)), field, v.expr));
        }
        "getfield" => {
            let idx = cp_index(ins);
            let obj = pop(stack);
            let (_, field, desc) = state.cf.member_ref(idx);
            stack.push(Val::new(
                format!("{}.{}", obj.expr, field),
                kind_of_desc(&desc),
            ));
        }
        "putfield" => {
            let idx = cp_index(ins);
            let v = pop(stack);
            let obj = pop(stack);
            let (_, field, _) = state.cf.member_ref(idx);
            stmt(out, format!("{}.{} = {};", obj.expr, field, v.expr));
        }

        // ---- invocations ----
        "invokevirtual" | "invokespecial" | "invokestatic" | "invokeinterface" => {
            if !invoke(state, ins, stack, out) {
                return Ok(false);
            }
        }
        "invokedynamic" => {
            if !invoke_dynamic(state, ins, stack, out) {
                return Ok(false);
            }
        }

        // ---- object/array creation ----
        "new" => {
            state.new_counter += 1;
            let id = state.new_counter;
            let idx = cp_index(ins);
            let class = to_dotted(&state.cf.class_name(idx));
            stack.push(Val {
                expr: format!("\u{1}NEW{id}"),
                kind: Kind::Ref,
                new_ref: Some(id),
                sb: class.ends_with("StringBuilder") || class.ends_with("StringBuffer"),
            });
        }
        "newarray" => {
            let count = pop(stack);
            let atype = match ins.operand {
                Operand::AType(t) => t,
                _ => 10,
            };
            let t = crate::java::opcodes::newarray_type(atype);
            stack.push(Val::new(format!("new {t}[{}]", count.expr), Kind::Ref));
        }
        "anewarray" => {
            let count = pop(stack);
            let idx = cp_index(ins);
            let t = binary_to_java(&state.cf.class_name(idx));
            stack.push(Val::new(
                format!("new {}[{}]", shorten(state.cf, &t), count.expr),
                Kind::Ref,
            ));
        }
        "multianewarray" => {
            let (idx, dims) = match ins.operand {
                Operand::Dims(i, d) => (i, d as usize),
                _ => (0, 1),
            };
            let mut sizes = Vec::with_capacity(dims);
            for _ in 0..dims {
                sizes.push(pop(stack).expr);
            }
            sizes.reverse();
            let t = binary_to_java(&state.cf.class_name(idx));
            let dims_str: String = sizes.iter().map(|s| format!("[{s}]")).collect();
            stack.push(Val::new(
                format!("new {}{}{}", shorten(state.cf, &t), dims_str, "[]".repeat(dims.saturating_sub(sizes.len()))),
                Kind::Ref,
            ));
        }
        "arraylength" => {
            let arr = pop(stack);
            stack.push(Val::new(format!("{}.length", arr.expr), Kind::Num));
        }
        "athrow" => {
            let v = pop(stack);
            // The catch-all (finally) handler's rethrow is compiler
            // machinery — the source-level `finally` has no throwable.
            if !state.finally_thrown.contains(&v.expr) {
                stmt(out, format!("throw {};", v.expr));
            }
        }
        "checkcast" => {
            let obj = pop(stack);
            let idx = cp_index(ins);
            let t = shorten(state.cf, &binary_to_java(&state.cf.class_name(idx)));
            stack.push(Val::new(format!("({})({})", t, obj.expr), obj.kind));
        }
        "instanceof" => {
            let obj = pop(stack);
            let idx = cp_index(ins);
            let t = shorten(state.cf, &binary_to_java(&state.cf.class_name(idx)));
            stack.push(Val::new(
                format!("{} instanceof {}", obj.expr, t),
                Kind::Bool,
            ));
        }
        "monitorenter" => {
            let obj = pop(stack);
            out.push(format!("        synchronized ({}) {{", obj.expr));
        }
        "monitorexit" => {
            // The monitor operand is consumed either way; the exceptional-
            // path release lives in the sync handler region — the
            // `synchronized (...) {` block's `}` was already emitted on the
            // normal path.
            let _obj = pop(stack);
            if !state.sync_exits.contains(&ins.offset) {
                out.push("        }".to_string());
            }
        }
        "wide" => return Ok(false),
        _ => return Ok(false),
    }
    Ok(true)
}

fn branch_target(ins: &Instruction) -> usize {
    match ins.operand {
        Operand::Target(t) => t,
        _ => 0,
    }
}

fn branch_stmt(out: &mut Vec<String>, cond: &str, target: usize) {
    out.push(format!(
        "        if ({cond}) goto {};",
        flow::label_name(target)
    ));
}

fn binop_val(stack: &mut Vec<Val>, op: &str) {
    let b = pop(stack);
    let a = pop(stack);
    let mut strs = vec![a.expr, b.expr];
    flow::binop(&mut strs, op);
    stack.push(Val::new(strs.pop().unwrap_or_default(), Kind::Num));
}

fn conv_val(stack: &mut Vec<Val>, ty: &str) {
    let a = pop(stack);
    let mut strs = vec![a.expr];
    flow::conv(&mut strs, ty);
    stack.push(Val::new(strs.pop().unwrap_or_default(), Kind::Num));
}

fn cp_index(ins: &Instruction) -> u16 {
    match ins.operand {
        Operand::Cp(i) => i,
        _ => 0,
    }
}

fn ldc_val(cf: &ClassFile, idx: u16) -> Val {
    match cf.constant(idx) {
        Const::Int(v) => Val::new(v.to_string(), Kind::Num),
        Const::Long(v) => Val::new(format!("{v}L"), Kind::Num),
        Const::Float(v) => Val::new(format_float(v), Kind::Num),
        Const::Double(v) => Val::new(format_double(v), Kind::Num),
        Const::Str(s) => Val::new(quote_java(&s), Kind::Str),
        Const::Class(c) => Val::new(
            format!("{}.class", shorten(cf, &binary_to_java(&c))),
            Kind::Ref,
        ),
        Const::Other(o) => Val::new(format!("/* {o} */ null"), Kind::Ref),
    }
}

fn load_local(locals: &HashMap<u16, LocalInfo>, slot: u16, op: &str) -> Val {
    let _ = op;
    match locals.get(&slot) {
        Some(info) => Val::new(info.name.clone(), Kind::Other),
        None => Val::new(format!("V_{slot}"), Kind::Other),
    }
}

fn store_local(state: &mut State, stack: &mut Vec<Val>, out: &mut Vec<String>, slot: u16) {
    // Exception handler entry: the JVM pushes the throwable; the catch
    // clause declares the variable, so emit nothing.
    if stack.is_empty() && state.exception_slots.contains_key(&slot) {
        return;
    }
    let v = pop(stack);
    let undeclared = state.locals.get(&slot).map(|l| !l.declared).unwrap_or(true);
    if undeclared {
        let name = state
            .locals
            .get(&slot)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| format!("V_{slot}"));
        let ty = state.locals.get(&slot).and_then(|l| l.ty.clone());
        let decl = match ty {
            Some(t) => format!("{t} {name} = {};", v.expr),
            None => format!("var {name} = {};", v.expr),
        };
        out.push(format!("        {decl}"));
        if let Some(info) = state.locals.get_mut(&slot) {
            info.declared = true;
        } else {
            state.locals.insert(
                slot,
                LocalInfo { name, ty: None, declared: true },
            );
        }
        return;
    }
    let lname = local_ref(&state.locals, slot);
    out.push(format!("        {lname} = {};", v.expr));
}

fn ensure_declared(state: &mut State, out: &mut Vec<String>, slot: u16, default: &str) {
    let declared = state.locals.get(&slot).map(|l| l.declared).unwrap_or(false);
    if !declared {
        let name = state
            .locals
            .get(&slot)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| format!("V_{slot}"));
        let ty = state.locals.get(&slot).and_then(|l| l.ty.clone());
        let decl = match ty {
            Some(t) => format!("{t} {name} = {default};"),
            None => format!("var {name} = {default};"),
        };
        out.push(format!("        {decl}"));
        if let Some(info) = state.locals.get_mut(&slot) {
            info.declared = true;
        }
    }
}

fn local_ref(locals: &HashMap<u16, LocalInfo>, slot: u16) -> String {
    locals
        .get(&slot)
        .map(|l| l.name.clone())
        .unwrap_or_else(|| format!("V_{slot}"))
}

/// True when an expression discarded by `pop` still has side effects.
fn is_side_effect(expr: &str) -> bool {
    expr.contains('(') || expr.contains("++") || expr.contains("--")
}

/// Render a method invocation. Returns false when unsupported.
fn invoke(state: &mut State, ins: &Instruction, stack: &mut Vec<Val>, out: &mut Vec<String>) -> bool {
    let idx = cp_index(ins);
    let (class, mname, desc) = state.cf.member_ref(idx);
    let class = to_dotted(&class);
    let (params, _ret_desc) =
        crate::java::descriptor::parse_method_descriptor(&desc).unwrap_or((Vec::new(), "V".into()));
    let ret_kind = kind_of_desc(&ret_desc_of(&desc));

    // Constructor calls come through invokespecial.
    if mname == "<init>" && ins.name == "invokespecial" {
        let args = pop_args(stack, params.len());
        let obj = pop(stack);
        if let Some(id) = obj.new_ref {
            let class_t = if obj.sb {
                class.clone()
            } else {
                shorten(state.cf, &class)
            };
            let expr = if obj.sb {
                // StringBuilder accumulator: start empty or with seed.
                if args.len() == 1 {
                    args[0].clone()
                } else {
                    String::new()
                }
            } else {
                format!("new {class_t}({})", args.join(", "))
            };
            let completed = Val {
                expr,
                kind: if obj.sb { Kind::Str } else { Kind::Ref },
                new_ref: None,
                sb: obj.sb,
            };
            // Replace every placeholder referring to this new instance
            // (dups included) with the completed expression.
            for v in stack.iter_mut() {
                if v.new_ref == Some(id) {
                    *v = completed.clone();
                }
            }
            return true;
        }
        // `this(...)` / `super(...)` constructor chaining.
        if obj.expr == "this" {
            if class == to_dotted(&state.cf.class_name(state.cf.this_class)) {
                if !args.is_empty() {
                    out.push(format!("        this({});", args.join(", ")));
                }
                return true;
            }
            // Explicit superclass constructor call — suppress the implicit
            // no-arg `super()` on java.lang.Object.
            if class == "java.lang.Object" && args.is_empty() {
                return true;
            }
            out.push(format!("        super({});", args.join(", ")));
            return true;
        }
        // Unknown receiver for <init> — render as a plain new.
        stack.push(Val::new(
            format!("new {}({})", shorten(state.cf, &class), args.join(", ")),
            Kind::Ref,
        ));
        return true;
    }

    // StringBuilder.append / toString (pre-javac-9 string concat).
    if (class == "java.lang.StringBuilder" || class == "java.lang.StringBuffer")
        && ins.name == "invokevirtual"
    {
        if mname == "append" {
            let args = pop_args(stack, params.len());
            let obj = pop(stack);
            if obj.sb {
                let expr = if obj.expr.is_empty() {
                    args.first().cloned().unwrap_or_default()
                } else {
                    format!("{} + {}", obj.expr, args.first().cloned().unwrap_or_default())
                };
                stack.push(Val { expr, kind: Kind::Str, new_ref: None, sb: true });
                return true;
            }
            // Non-accumulator receiver — render literally.
            stack.push(Val::new(
                format!("{}.append({})", obj.expr, args.join(", ")),
                Kind::Other,
            ));
            return true;
        }
        if mname == "toString" {
            let obj = pop(stack);
            if obj.sb {
                stack.push(Val { expr: obj.expr, kind: Kind::Str, new_ref: None, sb: false });
            } else {
                stack.push(Val::new(format!("{}.toString()", obj.expr), Kind::Str));
            }
            return true;
        }
    }

    let args = pop_args(stack, params.len());

    if ins.name == "invokestatic" {
        stack.push(Val::new(
            format!("{}.{}({})", shorten(state.cf, &class), mname, args.join(", ")),
            ret_kind,
        ));
        return true;
    }

    let obj = pop(stack);
    if obj.expr == "this" && ins.name == "invokespecial" && class != to_dotted(&state.cf.class_name(state.cf.this_class))
    {
        // Explicit `super.method()` call.
        stack.push(Val::new(
            format!("super.{}({})", mname, args.join(", ")),
            ret_kind,
        ));
        return true;
    }
    stack.push(Val::new(
        format!("{}.{}({})", obj.expr, mname, args.join(", ")),
        ret_kind,
    ));
    true
}

/// `invokedynamic`: reconstruct javac 9+ string concatenation from the
/// makeConcatWithConstants recipe; other bootstraps degrade to a comment.
fn invoke_dynamic(state: &mut State, ins: &Instruction, stack: &mut Vec<Val>, out: &mut Vec<String>) -> bool {
    let idx = cp_index(ins);
    let nat = match state.cf.constant_pool.get(idx as usize) {
        Some(crate::java::classfile::Cp::InvokeDynamic { nat, .. }) => *nat,
        _ => return false,
    };
    let (iname, desc) = state.cf.name_and_type(nat);
    let (params, _) =
        crate::java::descriptor::parse_method_descriptor(&desc).unwrap_or((Vec::new(), String::new()));

    if iname == "makeConcatWithConstants" {
        // Find the bootstrap method for this callsite and its recipe.
        let bsm_idx = match state.cf.constant_pool.get(idx as usize) {
            Some(crate::java::classfile::Cp::InvokeDynamic { attr, .. }) => *attr as usize,
            _ => return false,
        };
        let recipe = state
            .cf
            .bootstrap_methods
            .get(bsm_idx)
            .and_then(|bm| bm.arguments.first().copied())
            .map(|arg| match state.cf.constant(arg) {
                Const::Str(s) => s,
                _ => String::new(),
            })
            .unwrap_or_default();
        if recipe.is_empty() {
            return false;
        }

        let mut args: Vec<String> = (0..params.len()).map(|_| pop(stack).expr).collect();
        args.reverse();
        let mut arg_iter = args.into_iter();
        let mut const_iter = state
            .cf
            .bootstrap_methods
            .get(bsm_idx)
            .map(|bm| bm.arguments.iter().skip(1).cloned().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter();

        let mut parts: Vec<String> = Vec::new();
        let mut literal = String::new();
        for ch in recipe.chars() {
            match ch {
                '\u{1}' => {
                    if !literal.is_empty() {
                        parts.push(quote_java(&literal));
                        literal.clear();
                    }
                    parts.push(arg_iter.next().unwrap_or_else(|| "/*?*/".into()));
                }
                '\u{2}' => {
                    if !literal.is_empty() {
                        parts.push(quote_java(&literal));
                        literal.clear();
                    }
                    match const_iter.next().map(|c| state.cf.constant(c)) {
                        Some(Const::Str(s)) => parts.push(quote_java(&s)),
                        Some(Const::Int(v)) => parts.push(v.to_string()),
                        Some(Const::Long(v)) => parts.push(format!("{v}L")),
                        Some(c) => parts.push(const_literal(&c)),
                        None => parts.push("\"\"".into()),
                    }
                }
                other => literal.push(other),
            }
        }
        if !literal.is_empty() {
            parts.push(quote_java(&literal));
        }

        // Join with `+`, collapsing when there is a single component.
        let expr = if parts.is_empty() {
            "\"\"".to_string()
        } else {
            parts.join(" + ")
        };
        stack.push(Val::new(expr, Kind::Str));
        return true;
    }

    // Lambdas, method references, etc. — degrade gracefully.
    let _ = pop_args(stack, params.len());
    out.push(format!("        // invokedynamic {iname}"));
    stack.push(Val::new("null", Kind::Ref));
    true
}

// ---- exception markers ------------------------------------------------------

/// True when the handler at `h` is javac's synchronized-block machinery:
/// `astore E; aload M; monitorexit; ... athrow` (monitor re-release + rethrow).
fn is_sync_handler(instrs: &[Instruction], h: usize) -> bool {
    let is_astore = |op: u8| op == 0x3A || (0x4B..=0x4E).contains(&op);
    let is_aload = |op: u8| op == 0x19 || (0x2A..=0x2D).contains(&op);
    let mut it = instrs.iter().skip_while(|i| (i.offset as i64) < h as i64);
    let Some(a) = it.next() else { return false };
    if a.offset != h || !is_astore(a.op) {
        return false;
    }
    let Some(b) = it.next() else { return false };
    let Some(c) = it.next() else { return false };
    is_aload(b.op) && c.op == 0xC3
}

/// Insert `try`/`catch`/`finally` markers based on the exception table.
///
/// Unlike .NET EH clauses, Java handler regions have no explicit length — a
/// handler region ends where the next handler starts (or at the end of the
/// method). javac also emits bookkeeping ranges we must not render:
/// * a self-covering range (`start == handler`) protects the finally body
///   itself and adds no visible structure;
/// * a range whose start is another clause's handler protects the catch
///   body — already expressed by the surrounding `finally` clause;
/// * several ranges share one handler (dedup to the widest try).
fn insert_exception_markers(
    out: &mut Vec<String>,
    offset_to_line: &HashMap<usize, usize>,
    code: &Code,
    cf: &ClassFile,
    skip_handlers: &HashSet<usize>,
) {
    if code.exception_table.is_empty() {
        return;
    }
    let line_of = |off: usize| -> Option<usize> { offset_to_line.get(&off).copied() };

    let mut handler_pcs: Vec<usize> = code
        .exception_table
        .iter()
        .map(|e| e.handler_pc as usize)
        .collect();
    handler_pcs.sort();
    handler_pcs.dedup();
    let handler_end = |h: usize| -> usize {
        match handler_pcs.iter().position(|&s| s == h) {
            Some(i) if i + 1 < handler_pcs.len() => handler_pcs[i + 1],
            _ => code.code.len(),
        }
    };

    // Distinct clauses: dedupe by (handler, catch type), keeping the widest
    // try range (entries sorted by handler then start).
    let mut entries = code.exception_table.clone();
    entries.sort_by_key(|e| (e.handler_pc, e.start_pc));
    entries.dedup_by(|a, b| a.handler_pc == b.handler_pc && a.catch_type == b.catch_type);

    // Build position-anchored insertions.
    let mut actions: Vec<(usize, String)> = Vec::new();
    let mut trailing_close = false;
    let mut any_clause = false;
    let mut open_try: Option<usize> = None;
    let mut last_handler = 0usize;
    for e in &entries {
        let start = e.start_pc as usize;
        let h = e.handler_pc as usize;
        if skip_handlers.contains(&h) {
            continue;
        }
        if start == h || handler_pcs.contains(&start) {
            continue;
        }
        any_clause = true;
        last_handler = h;
        if open_try != Some(start) {
            if let Some(tl) = line_of(start) {
                actions.push((tl, "        try {".to_string()));
            }
            open_try = Some(start);
        }
        let header = if e.catch_type == 0 {
            "        finally {".to_string()
        } else {
            let t = shorten(cf, &to_dotted(&cf.class_name(e.catch_type)));
            format!("        catch ({t} e) {{")
        };
        if let Some(hl) = line_of(h) {
            actions.push((hl, header));
            actions.push((hl, "        }".to_string()));
        } else {
            trailing_close = true;
            actions.push((out.len(), header));
        }
    }
    // Close the last handler region at the next handler start / method end.
    if any_clause {
        let end_off = handler_end(last_handler);
        match line_of(end_off) {
            Some(l) => actions.push((l, "        }".to_string())),
            None => trailing_close = true,
        }
        if trailing_close {
            actions.push((out.len(), "        }".to_string()));
        }
    }

    // Insert from the bottom up so indices stay valid; at the same line the
    // later-listed text must be inserted first (each insert pushes the rest
    // down), so sort stably by descending line and iterate in order, but
    // insert brace-before-header by listing header first.
    actions.sort_by_key(|(line, _)| std::cmp::Reverse(*line));
    for (line, text) in actions {
        let at = line.min(out.len());
        out.insert(at, text);
    }
}

/// Fold javac's materialized booleans back into direct returns:
///   if (COND) { var T = 1; } else { T = 0; }
///   return T;
/// → `return COND;`
/// javac compiles `return x > 0;` to iconst_1/iconst_0 around branches; the
/// condition polarity already matches the then-branch assigning 1.
/// Fold javac's lowered enhanced-for loop back into source form:
///   var V_it = EXPR.iterator();
///   while (V_it.hasNext()) {
///       var V_el = V_it.next();
///       ... body ...
///   }
/// → `for (var V_el : EXPR) { body }`
/// Uses `var` for the element type — no type inference required. Runs after
/// the while-loop passes, before `restructure_for_loops`.
fn restructure_foreach(out: &mut [String]) {
    let mut i = 0;
    while i + 3 < out.len() {
        // Iterator init: `var V_it = EXPR.iterator();`
        let init = out[i].trim().to_string();
        let Some((it, expr)) = init
            .strip_prefix("var ")
            .and_then(|s| s.strip_suffix(';'))
            .and_then(|s| s.split_once(" = "))
            .map(|(v, e)| (v.trim().to_string(), e.trim().to_string()))
        else {
            i += 1;
            continue;
        };
        let Some(iterable) = expr.strip_suffix(".iterator()") else {
            i += 1;
            continue;
        };

        // Loop header: `while (...V_it.hasNext()...) {`
        let while_line = out[i + 1].trim();
        if !while_line.starts_with("while (")
            || !while_line.contains(&format!("{it}.hasNext()"))
            || !while_line.ends_with('{')
        {
            i += 1;
            continue;
        }

        // First body statement: `[var ]V_el = V_it.next();`
        // (skip blank/comment lines left by earlier passes).
        let Some(next_idx) = (i + 2..out.len()).find(|&j| {
            let t = out[j].trim();
            !t.is_empty() && !t.starts_with("//")
        }) else {
            i += 1;
            continue;
        };
        let first_body = out[next_idx].trim().to_string();
        let stripped = first_body.strip_suffix(';').unwrap_or(&first_body);
        let no_var = stripped.strip_prefix("var ").unwrap_or(stripped);
        let Some((lhs, rhs)) = no_var.split_once(" = ") else {
            i += 1;
            continue;
        };
        if rhs.trim() != format!("{it}.next()") {
            i += 1;
            continue;
        }
        let elem = lhs.trim();

        // Matching close brace of the while block.
        let Some(close) = find_matching_brace(out, i + 1) else {
            i += 1;
            continue;
        };

        out[i] = String::new();
        out[i + 1] = format!("        for (var {elem} : {iterable}) {{");
        out[next_idx] = String::new();
        i = close + 1;
    }
}

/// Index of the `}` closing the block opened at `open_idx` (a line ending
/// with `{`). Lines that both start with `}` and end with `{` (`} else {`)
/// are net-zero.
fn find_matching_brace(out: &[String], open_idx: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (j, line) in out.iter().enumerate().skip(open_idx) {
        let t = line.trim();
        if t.starts_with('}') {
            depth -= 1;
            if depth == 0 && t == "}" {
                return Some(j);
            }
        }
        if t.ends_with('{') {
            depth += 1;
        }
    }
    None
}

fn restructure_boolean_returns(out: &mut [String]) {
    let mut i = 0;
    while i + 5 < out.len() {
        let header = out[i].trim();
        if !header.starts_with("if (") || !header.ends_with("{") {
            i += 1;
            continue;
        }
        let cond = match header[4..].rfind(')') {
            Some(p) => &header[4..4 + p],
            None => {
                i += 1;
                continue;
            }
        };

        // Find `} else {` and the assignment lines, skipping comments.
        let find_assign = |from: usize, to: usize, val: &str| -> Option<(usize, String)> {
            for (j, line) in out.iter().enumerate().take(to).skip(from) {
                let t = line.trim();
                if t.starts_with("//") {
                    continue;
                }
                if let Some((lhs, rhs)) = t.split_once(" = ")
                    && rhs.trim_end_matches(';').trim() == val {
                        let name = lhs.trim().trim_start_matches("var ").trim();
                        return Some((j, name.to_string()));
                    }
                return None;
            }
            None
        };

        let else_idx = match (i + 1..out.len()).position(|j| out[j].trim() == "} else {") {
            Some(p) => i + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };
        let (_then_idx, temp) = match find_assign(i + 1, else_idx, "1") {
            Some(x) => x,
            None => {
                i += 1;
                continue;
            }
        };
        let close_idx = match (else_idx + 1..out.len()).position(|j| out[j].trim() == "}") {
            Some(p) => else_idx + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };
        let (else_assign_idx, temp2) = match find_assign(else_idx + 1, close_idx, "0") {
            Some(x) => x,
            None => {
                i += 1;
                continue;
            }
        };
        if temp != temp2 {
            i += 1;
            continue;
        }
        // Next non-empty line must be `return T;`.
        let ret_idx = match (close_idx + 1..out.len()).find(|&j| !out[j].trim().is_empty()) {
            Some(j) => j,
            None => {
                i += 1;
                continue;
            }
        };
        let _ = else_assign_idx;
        if out[ret_idx].trim() != format!("return {temp};") {
            i += 1;
            continue;
        }

        out[i] = format!("        return {cond};");
        for line in out[i + 1..=ret_idx].iter_mut() {
            *line = String::new();
        }
        i += 1;
    }
}

/// Fold ternary joins materialized through join temps:
///   if (COND) { var T = E1; } else { T = E2; }
///   ... uses of T ...
/// → `var T = COND ? E1 : E2;`
/// Needed for valid Java: `var` is block-scoped, so a temp declared inside
/// the then-branch is invisible at the join where javac consumes it.
fn restructure_ternary(out: &mut [String]) {
    let mut i = 0;
    while i + 3 < out.len() {
        let header = out[i].trim();
        if !header.starts_with("if (") || !header.ends_with('{') {
            i += 1;
            continue;
        }
        let cond = match header[4..].rfind(')') {
            Some(p) => header[4..4 + p].trim(),
            None => {
                i += 1;
                continue;
            }
        };

        let else_idx = match (i + 1..out.len()).position(|j| out[j].trim() == "} else {") {
            Some(p) => i + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };
        // Then-branch: single assignment (skip comments).
        let (then_line, then_name, then_expr, then_var) =
            match single_assign(out, i + 1, else_idx) {
                Some(x) => x,
                None => {
                    i += 1;
                    continue;
                }
            };
        let close_idx = match (else_idx + 1..out.len()).position(|j| out[j].trim() == "}") {
            Some(p) => else_idx + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };
        let (_, else_name, else_expr, _) = match single_assign(out, else_idx + 1, close_idx) {
            Some(x) => x,
            None => {
                i += 1;
                continue;
            }
        };
        if then_name != else_name || then_name.is_empty() {
            i += 1;
            continue;
        }

        let decl = if then_var { "var " } else { "" };
        out[i] = format!("        {decl}{then_name} = {cond} ? {then_expr} : {else_expr};");
        let _ = then_line;
        for line in out[i + 1..=close_idx].iter_mut() {
            *line = String::new();
        }
        i += 1;
    }
}

/// Find a single assignment line in [from, to), skipping comments.
/// Returns (line idx, target name, expr, declared-with-var).
fn single_assign(
    out: &[String],
    from: usize,
    to: usize,
) -> Option<(usize, String, String, bool)> {
    for (j, line) in out.iter().enumerate().take(to).skip(from) {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        if let Some((lhs, rhs)) = t.split_once(" = ") {
            let mut declared = false;
            let mut name = lhs.trim();
            if let Some(rest) = name.strip_prefix("var ") {
                declared = true;
                name = rest.trim();
            }
            if name.ends_with(';') {
                return None;
            }
            let expr = rhs.trim_end_matches(';').trim();
            return Some((j, name.to_string(), expr.to_string(), declared));
        }
        return None;
    }
    None
}

// ---- helpers ----------------------------------------------------------------

fn kind_of_desc(desc: &str) -> Kind {
    match desc.chars().next() {
        Some('Z') => Kind::Bool,
        Some('B' | 'C' | 'D' | 'F' | 'I' | 'J' | 'S') => Kind::Num,
        Some('L' | '[') => Kind::Ref,
        _ => Kind::Other,
    }
}

/// The raw return descriptor of a method descriptor string.
fn ret_desc_of(desc: &str) -> String {
    match desc.rfind(')') {
        Some(i) => desc[i + 1..].to_string(),
        None => "V".into(),
    }
}

/// Parameter names for a method: LocalVariableTable names when available,
/// otherwise `arg0..argN`.
fn param_names(m: &Member, params: &[String], is_static: bool) -> Vec<String> {
    let lvt: HashMap<u16, String> = m
        .code()
        .map(|c| {
            c.local_vars
                .iter()
                .map(|lv| (lv.index, lv.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut slot = if is_static { 0 } else { 1 };
    let mut names = Vec::with_capacity(params.len());
    for (i, p) in params.iter().enumerate() {
        let _ = i;
        names.push(lvt.get(&slot).cloned().unwrap_or_else(|| format!("arg{}", slot - if is_static { 0 } else { 1 })));
        slot += if *p == "long" || *p == "double" { 2 } else { 1 };
    }
    names
}

fn lvt_name(code: &Code, slot: u16) -> Option<String> {
    code.local_vars
        .iter()
        .find(|lv| lv.index == slot)
        .map(|lv| lv.name.clone())
}

/// Binary type name (Class entry) → dotted Java type, handling array forms.
fn binary_to_java(binary: &str) -> String {
    if binary.starts_with('[') {
        crate::java::descriptor::parse_field_descriptor(binary).unwrap_or_else(|_| binary.into())
    } else {
        to_dotted(binary)
    }
}

/// Shorten `java.lang.X` and same-package class names to their simple form.
fn shorten(cf: &ClassFile, dotted: &str) -> String {
    if let Some(rest) = dotted.strip_prefix("java.lang.") {
        if !rest.contains('.') {
            return rest.to_string();
        }
        // java.lang.reflect.X etc — keep fully qualified.
        return dotted.to_string();
    }
    let pkg = cf.package();
    if !pkg.is_empty()
        && let Some(rest) = dotted.strip_prefix(&format!("{pkg}.")) {
            return rest.to_string();
        }
    dotted.to_string()
}

fn to_dotted(binary: &str) -> String {
    crate::java::classfile::to_dotted(binary)
}

fn quote_java(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn format_float(v: f32) -> String {
    if v == v.trunc() && v.is_finite() && v.abs() < 1e7 {
        format!("{v:.1}f")
    } else {
        format!("{v}f")
    }
}

fn format_double(v: f64) -> String {
    if v == v.trunc() && v.is_finite() && v.abs() < 1e7 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

fn const_literal(c: &Const) -> String {
    match c {
        Const::Int(v) => v.to_string(),
        Const::Long(v) => format!("{v}L"),
        Const::Float(v) => format_float(*v),
        Const::Double(v) => format_double(*v),
        Const::Str(s) => quote_java(s),
        Const::Class(name) => format!("{}.class", binary_to_java(name)),
        Const::Other(o) => format!("/* {o} */"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreach_reconstruction() {
        let mut out = vec![
            "        var V_3 = this.items.iterator();".into(),
            "        while (V_3.hasNext()) {".into(),
            "            var V_4 = V_3.next();".into(),
            "            V_2.add(arg0.apply(V_4));".into(),
            "        }".into(),
            "        return V_2;".into(),
        ];
        restructure_foreach(&mut out);
        flow::drop_empty_lines(&mut out);
        assert_eq!(out[0], "        for (var V_4 : this.items) {");
        assert_eq!(out[1], "            V_2.add(arg0.apply(V_4));");
        assert_eq!(out[2], "        }");
        assert_eq!(out[3], "        return V_2;");
    }

    #[test]
    fn foreach_with_nested_if_else_keeps_structure() {
        let mut out = vec![
            "        var V_1 = list.iterator();".into(),
            "        while (V_1.hasNext()) {".into(),
            "            var V_2 = V_1.next();".into(),
            "            if (V_2 > 0) {".into(),
            "                total += V_2;".into(),
            "            } else {".into(),
            "                total -= V_2;".into(),
            "            }".into(),
            "        }".into(),
        ];
        restructure_foreach(&mut out);
        flow::drop_empty_lines(&mut out);
        assert_eq!(out[0], "        for (var V_2 : list) {");
        assert_eq!(out[3], "            } else {");
        assert_eq!(out[5], "            }");
        assert_eq!(out[6], "        }");
    }

    #[test]
    fn foreach_skips_non_iterator_patterns() {
        let mut out = vec![
            "        var V_1 = makeThing();".into(),
            "        while (V_1.hasNext()) {".into(),
            "            var V_2 = V_1.next();".into(),
            "        }".into(),
        ];
        restructure_foreach(&mut out);
        // untouched — no `.iterator()` init
        assert_eq!(out[0], "        var V_1 = makeThing();");
    }

    #[test]
    fn find_matching_brace_handles_else() {
        let out = vec![
            "        if (x) {".into(),
            "            a();".into(),
            "        } else {".into(),
            "            b();".into(),
            "        }".into(),
            "        c();".into(),
        ];
        // opening the if at 0: `} else {` must not close the block
        assert_eq!(find_matching_brace(&out, 0), Some(4));
    }
}
