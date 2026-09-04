use clap::Parser;
use std::path::PathBuf;
use transcode::{cil, decompile, error, metadata, output, pe};

/// transcode - a .NET decompiler written in Rust.
#[derive(Parser, Debug)]
#[command(name = "transcode", version, about)]
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

    if cli.il {
        let types = decompile_il(&reader)?;
        let n = output::write_types(&cli.output, &types)?;
        println!("Wrote {n} IL file(s) to {}", cli.output.display());
        return Ok(());
    }

    let types = decompile::decompile_assembly(&reader)?;
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

/// Produce IL disassembly files, one per type.
fn decompile_il(reader: &metadata::Reader<'_>) -> error::Result<Vec<decompile::DecompiledType>> {
    let mut out = Vec::new();
    let type_defs = reader.tables.get(metadata::tbl::TYPEDEF);
    for (i, row) in type_defs.iter().enumerate() {
        let row_idx = (i + 1) as u32;
        let name = reader.type_def_name(row);
        if name == "<Module>" {
            continue;
        }
        let ns = reader.type_def_namespace(row);
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
