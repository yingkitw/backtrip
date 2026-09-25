use clap::Parser;
use std::path::PathBuf;
use backtrip::{cil, decompile, error, java, metadata, output, pe};

/// backtrip - a .NET IL / JVM bytecode decompiler and disassembler in Rust.
#[derive(Parser, Debug)]
#[command(name = "backtrip", version, about)]
struct Cli {
    /// Path to the .NET assembly (.dll/.exe), Java class file (.class), or a
    /// directory when --recursive is used.
    assembly: PathBuf,

    /// Output directory for decompiled files.
    #[arg(short, long, default_value = "decompiled")]
    output: PathBuf,

    /// Emit CIL disassembly (ildasm-style) for .NET inputs, or javap-style
    /// bytecode disassembly for Java class files.
    #[arg(long)]
    il: bool,

    /// List types in the input and exit.
    #[arg(long)]
    list: bool,

    /// Decompile only the type whose simple or fully-qualified name matches.
    #[arg(long = "type", name = "type")]
    type_name: Option<String>,

    /// Print the matched type to stdout instead of writing files
    /// (requires --type).
    #[arg(long)]
    stdout: bool,

    /// Recursively decompile all .dll/.exe/.class files in a directory.
    #[arg(long)]
    recursive: bool,

    /// Emit a JSON model of the assembly (types/methods/fields) to stdout.
    #[arg(long)]
    json: bool,

    /// Detect obfuscation indicators (non-printable names, control-flow
    /// flattening, string encryption) and emit warnings.
    #[arg(long)]
    detect_obfuscation: bool,

    /// Verify decompiled output against metadata (check all types/methods/
    /// fields appear in the output).
    #[arg(long)]
    verify: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> error::Result<()> {
    let cli = Cli::parse();

    // Recursive mode: decompile all .dll/.exe in a directory.
    if cli.recursive {
        return run_recursive(&cli);
    }

    decompile_one(&cli)
}

/// Decompile a single input file (.NET assembly, Java class file, or jar).
/// The format is detected from the magic bytes: `MZ` → .NET PE,
/// `0xCAFEBABE` → JVM class file, `PK\x03\x04` → jar archive.
fn decompile_one(cli: &Cli) -> error::Result<()> {
    let data = std::fs::read(&cli.assembly)?;
    if data.len() >= 4
        && (data[..4] == [0xCA, 0xFE, 0xBA, 0xBE] || data[..4] == [0x50, 0x4B, 0x03, 0x04])
    {
        return decompile_java(cli, &data);
    }
    let pe = pe::PeImage::parse(data)?;
    let (root, tables) = metadata::load(&pe)?;
    let reader = metadata::Reader::new(&pe, &root, &tables)?;

    if cli.list {
        list_types(&reader)?;
        return Ok(());
    }

    if cli.json {
        let json = decompile::json::assembly_to_json(&reader)?;
        print!("{json}");
        return Ok(());
    }

    if cli.detect_obfuscation {
        let warnings = decompile::obfuscation::detect_obfuscation(&reader)?;
        print!("{}", decompile::obfuscation::format_warnings(&warnings));
        return Ok(());
    }

    if cli.verify {
        let types = decompile::decompile_assembly(&reader)?;
        let results = decompile::verify::verify(&reader, &types)?;
        print!("{}", decompile::verify::format_results(&results));
        return Ok(());
    }

    if cli.stdout && cli.type_name.is_none() {
        return Err(error::Error::Usage(
            "--stdout requires --type <NAME>".into(),
        ));
    }

    if cli.il {
        let types = decompile_il(&reader, cli.type_name.as_deref())?;
        if types.is_empty() {
            return Err(error::Error::NotFound(format!(
                "no type matching '{}'",
                cli.type_name.as_deref().unwrap_or("")
            )));
        }
        if cli.stdout {
            for t in &types {
                print!("{}", t.source);
            }
            return Ok(());
        }
        let n = output::write_types(&cli.output, &types)?;
        println!("Wrote {n} IL file(s) to {}", cli.output.display());
        return Ok(());
    }

    let types = if let Some(q) = cli.type_name.as_deref() {
        match decompile::decompile_type_by_name(&reader, q)? {
            Some(t) => vec![t],
            None => {
                return Err(error::Error::NotFound(format!(
                    "no type matching '{q}'"
                )));
            }
        }
    } else {
        decompile::decompile_assembly(&reader)?
    };
    if cli.stdout {
        for t in &types {
            print!("{}", t.source);
        }
        return Ok(());
    }
    let n = output::write_types(&cli.output, &types)?;
    println!("Wrote {n} file(s) to {}", cli.output.display());
    Ok(())
}

/// Decompile a JVM class file: Java source (default) or javap-style
/// disassembly (--il). Mirrors the .NET CLI surface where applicable.
fn decompile_java(cli: &Cli, data: &[u8]) -> error::Result<()> {
    let is_jar = data.len() >= 4 && data[..4] == [0x50, 0x4B, 0x03, 0x04];

    if cli.list {
        if is_jar {
            for name in decompile::java::jar_class_names(data)? {
                println!("{name}");
            }
        } else {
            println!("{}", decompile::java::list_name(data)?);
        }
        return Ok(());
    }
    if cli.json {
        return Err(error::Error::Usage(
            "--json is not supported for Java class files".into(),
        ));
    }
    if cli.detect_obfuscation {
        return Err(error::Error::Usage(
            "--detect-obfuscation is not supported for Java class files".into(),
        ));
    }
    if cli.verify {
        return Err(error::Error::Usage(
            "--verify is not supported for Java class files".into(),
        ));
    }
    // A class file contains exactly one class, so --stdout works without
    // --type here (unlike assemblies, where --type selects among many).

    // Name filter: match the simple or fully-qualified class name.
    let matches_filter = |name: &str, pkg: &str| -> bool {
        match cli.type_name.as_deref() {
            None => true,
            Some(q) => name == q || format!("{pkg}.{name}") == q,
        }
    };

    // Parse every class input into (classfile, source) pairs. Broken jar
    // entries are skipped instead of failing the whole archive.
    let mut units: Vec<(java::ClassFile, decompile::DecompiledType)> = Vec::new();
    if is_jar {
        for entry in java::jar::read_entries(data)? {
            if !entry.name.ends_with(".class") {
                continue;
            }
            if let Ok(cf) = java::ClassFile::parse(&entry.data)
                && let Ok(t) = decompile::java::decompile_class(&cf) {
                    units.push((cf, t));
                }
        }
    } else {
        let cf = java::ClassFile::parse(data)?;
        let t = decompile::java::decompile_class_file(data)?;
        units.push((cf, t));
    }

    if cli.il {
        let mut types = Vec::new();
        for (cf, _) in &units {
            if !matches_filter(&cf.simple_name(), &cf.package()) {
                continue;
            }
            let source = java::disasm::disassemble_class(cf)?;
            types.push(decompile::DecompiledType {
                file_name: java_disasm_file_name(cf),
                source,
            });
        }
        if types.is_empty() {
            return Err(error::Error::NotFound(format!(
                "no class matching '{}'",
                cli.type_name.as_deref().unwrap_or("")
            )));
        }
        if cli.stdout {
            for t in &types {
                print!("{}", t.source);
            }
            return Ok(());
        }
        let n = output::write_types(&cli.output, &types)?;
        println!("Wrote {n} disassembly file(s) to {}", cli.output.display());
        return Ok(());
    }

    let types: Vec<decompile::DecompiledType> = units
        .iter()
        .filter(|(cf, _)| matches_filter(&cf.simple_name(), &cf.package()))
        .map(|(_, t)| t.clone())
        .collect();
    if types.is_empty() {
        return Err(error::Error::NotFound(format!(
            "no class matching '{}'",
            cli.type_name.as_deref().unwrap_or("")
        )));
    }
    if cli.stdout {
        for t in &types {
            print!("{}", t.source);
        }
        return Ok(());
    }
    let n = output::write_types(&cli.output, &types)?;
    println!("Wrote {n} file(s) to {}", cli.output.display());
    Ok(())
}

fn java_disasm_file_name(cf: &java::ClassFile) -> String {
    let simple = cf.simple_name();
    let pkg = cf.package();
    let stem = if pkg.is_empty() {
        simple
    } else {
        format!("{}_{}", pkg.replace('.', "_"), simple)
    };
    format!("{stem}.jbc")
}

/// Recursively find and decompile all .dll/.exe files in a directory.
fn run_recursive(cli: &Cli) -> error::Result<()> {
    if !cli.assembly.is_dir() {
        return Err(error::Error::Usage(
            "--recursive requires a directory path".into(),
        ));
    }

    // Collect all .dll and .exe files.
    let mut assemblies: Vec<PathBuf> = Vec::new();
    collect_assemblies(&cli.assembly, &mut assemblies)?;
    assemblies.sort();

    if assemblies.is_empty() {
        eprintln!("No .dll or .exe files found in {}", cli.assembly.display());
        return Ok(());
    }

    let total_files = 0;
    let mut total_assemblies = 0;
    let mut errors = 0;

    for asm_path in &assemblies {
        let rel = asm_path.strip_prefix(&cli.assembly).unwrap_or(asm_path);
        let stem = rel.with_extension("").to_string_lossy().replace('/', "_");
        let out_dir = cli.output.join(&stem);

        // Clone the CLI args with this assembly and output dir.
        let sub_cli = Cli {
            assembly: asm_path.clone(),
            output: out_dir,
            il: cli.il,
            list: false,
            type_name: cli.type_name.clone(),
            stdout: false,
            recursive: false,
            json: false,
            detect_obfuscation: false,
            verify: false,
        };

        match decompile_one(&sub_cli) {
            Ok(_) => {
                total_assemblies += 1;
            }
            Err(e) => {
                errors += 1;
                eprintln!("Failed to decompile {}: {e}", asm_path.display());
            }
        }
    }

    println!(
        "Decompiled {total_assemblies} assemblies ({} errors) to {}",
        errors,
        cli.output.display()
    );
    let _ = total_files;
    Ok(())
}

/// Recursively collect .dll and .exe files from a directory.
fn collect_assemblies(dir: &PathBuf, out: &mut Vec<PathBuf>) -> error::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_assemblies(&path, out)?;
        } else if let Some(ext) = path.extension() {
            let ext = ext.to_ascii_lowercase();
            if ext == "dll" || ext == "exe" || ext == "class" || ext == "jar" {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn list_types(reader: &metadata::Reader<'_>) -> error::Result<()> {
    let type_defs = reader.tables.get(metadata::tbl::TYPEDEF);
    for (i, row) in type_defs.iter().enumerate() {
        let name = reader.type_def_name(row);
        if name == "<Module>" {
            continue;
        }
        let ns = reader.type_def_namespace(row);
        let row_idx = (i + 1) as u32;
        let nested = reader.nested_parent(row_idx).map(|p| {
            let parent = &reader.tables.get(metadata::tbl::TYPEDEF)[p as usize - 1];
            reader.type_def_name(parent)
        });
        if let Some(parent) = nested {
            println!("{parent}/{name}");
        } else if ns.is_empty() {
            println!("{name}");
        } else {
            println!("{ns}.{name}");
        }
    }
    Ok(())
}

/// Produce IL disassembly files, one per type. If `filter` is given, only the
/// type whose simple or fully-qualified name matches is included.
fn decompile_il(reader: &metadata::Reader<'_>, filter: Option<&str>) -> error::Result<Vec<decompile::DecompiledType>> {
    let mut out = Vec::new();
    let type_defs = reader.tables.get(metadata::tbl::TYPEDEF);
    for (i, row) in type_defs.iter().enumerate() {
        let row_idx = (i + 1) as u32;
        let name = reader.type_def_name(row);
        if name == "<Module>" {
            continue;
        }
        let ns = reader.type_def_namespace(row);
        if let Some(q) = filter {
            let full = if ns.is_empty() {
                name.clone()
            } else {
                format!("{ns}.{name}")
            };
            if name != q && full != q {
                continue;
            }
        }
        let source = il_for_type(reader, row_idx)?;
        let file_name = {
            let clean = name.replace(['`', '/'], "_");
            if ns.is_empty() {
                format!("{clean}.il")
            } else {
                format!("{}_{clean}.il", ns.replace('.', "_"))
            }
        };
        out.push(decompile::DecompiledType { file_name, source });
    }
    Ok(out)
}

fn il_for_type(reader: &metadata::Reader<'_>, row_idx: u32) -> error::Result<String> {
    let row = &reader.tables.get(metadata::tbl::TYPEDEF)[row_idx as usize - 1];
    let name = reader.type_def_name(row);
    let ns = reader.type_def_namespace(row);
    let mut s = String::new();
    s.push_str(&format!(".class {ns} {name}\n{{\n"));

    // Fields
    for fi in reader.type_field_rows(row_idx) {
        let f = &reader.tables.get(metadata::tbl::FIELD)[fi];
        let ftype = reader.field_type(f).map(|t| reader.type_name(&t)).unwrap_or_else(|_| "object".into());
        s.push_str(&format!("  .field {} {}\n", ftype, reader.field_name(f)));
    }

    // Methods
    for mi in reader.type_method_rows(row_idx) {
        let m = &reader.tables.get(metadata::tbl::METHODDEF)[mi];
        let mname = reader.method_name(m);
        let sig = reader.method_sig(m)?;
        let ret = reader.type_name(&sig.ret_type);
        let params: Vec<String> = sig.param_types.iter().enumerate().map(|(i, t)| {
            format!("{} arg{i}", reader.type_name(t))
        }).collect();
        s.push_str(&format!("  .method {} {}({})\n  {{\n", ret, mname, params.join(", ")));
        let rva = reader.method_rva(m);
        if let Some(body) = reader.method_body(rva)? {
            // Emit a .locals header when the method has locals.
            let locals = reader.local_types(body.local_token);
            if !locals.is_empty() {
                let decls: Vec<String> = locals.iter().enumerate()
                    .map(|(i, t)| format!("{} V_{i}", reader.type_name(t)))
                    .collect();
                s.push_str(&format!("    .locals init ({})\n", decls.join(", ")));
            }
            let il = cil::disasm::disassemble(reader, &body.code)?;
            for line in il.lines() {
                s.push_str(&format!("    {line}\n"));
            }
        } else {
            s.push_str("    // no body (abstract / extern / pinvoke)\n");
        }
        s.push_str("  }\n");
    }
    s.push_str("}\n");
    Ok(s)
}
