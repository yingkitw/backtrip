use clap::Parser;
use std::path::PathBuf;
use roundtrip::{cil, decompile, error, metadata, output, pe};

/// roundtrip - a .NET decompiler written in Rust.
#[derive(Parser, Debug)]
#[command(name = "roundtrip", version, about)]
struct Cli {
    /// Path to the .NET assembly (.dll / .exe) to decompile.
    assembly: PathBuf,

    /// Output directory for decompiled files.
    #[arg(short, long, default_value = "decompiled")]
    output: PathBuf,

    /// Emit CIL disassembly (ildasm-style) instead of C# source.
    #[arg(long)]
    il: bool,

    /// List types in the assembly and exit.
    #[arg(long)]
    list: bool,

    /// Decompile only the type whose simple or fully-qualified name matches.
    #[arg(long = "type", name = "type")]
    type_name: Option<String>,

    /// Print the matched type to stdout instead of writing files
    /// (requires --type).
    #[arg(long)]
    stdout: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> error::Result<()> {
    let cli = Cli::parse();

    let data = std::fs::read(&cli.assembly)?;
    let pe = pe::PeImage::parse(data)?;
    let (root, tables) = metadata::load(&pe)?;
    let reader = metadata::Reader::new(&pe, &root, &tables)?;

    if cli.list {
        list_types(&reader)?;
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
            let clean = name.replace('`', "_").replace('/', "_");
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
