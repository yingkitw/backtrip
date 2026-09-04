//! JSON output mode — emit a structured model of the assembly for machine
//! consumption.

use crate::error::Result;
use crate::metadata::reader::Reader;
use crate::metadata::tables::tbl;

/// Escape a string for JSON output.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Produce a JSON model of the entire assembly.
pub fn assembly_to_json(reader: &Reader<'_>) -> Result<String> {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"types\": [\n");

    let type_defs = reader.tables.get(tbl::TYPEDEF);
    let mut first = true;
    for (i, row) in type_defs.iter().enumerate() {
        let name = reader.type_def_name(row);
        if name == "<Module>" {
            continue;
        }
        let ns = reader.type_def_namespace(row);
        let row_idx = (i + 1) as u32;

        if !first {
            s.push_str(",\n");
        }
        first = false;

        s.push_str("    {\n");
        s.push_str(&format!("      \"name\": {},\n", esc(&name)));
        s.push_str(&format!("      \"namespace\": {},\n", esc(&ns)));

        // Fields
        s.push_str("      \"fields\": [");
        let field_rows: Vec<usize> = reader.type_field_rows(row_idx).collect();
        for (fi, fr) in field_rows.iter().enumerate() {
            let f = &reader.tables.get(tbl::FIELD)[*fr];
            let fname = reader.field_name(f);
            let ftype = reader.field_type(f)
                .map(|t| crate::decompile::csharp::strip_system_pub(&reader.type_name(&t)))
                .unwrap_or_else(|_| "object".into());
            if fi > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "\n        {{\"name\": {}, \"type\": {}}}",
                esc(&fname), esc(&ftype)
            ));
        }
        if !field_rows.is_empty() {
            s.push_str("\n      ");
        }
        s.push_str("],\n");

        // Methods
        s.push_str("      \"methods\": [");
        let method_rows: Vec<usize> = reader.type_method_rows(row_idx).collect();
        for (mi, mr) in method_rows.iter().enumerate() {
            let m = &reader.tables.get(tbl::METHODDEF)[*mr];
            let mname = reader.method_name(m);
            let sig = reader.method_sig(m);
            let (ret_type, params) = match &sig {
                Ok(sig) => {
                    let ret = crate::decompile::csharp::strip_system_pub(&reader.type_name(&sig.ret_type));
                    let params: Vec<String> = sig.param_types.iter().map(|t| {
                        crate::decompile::csharp::strip_system_pub(&reader.type_name(t))
                    }).collect();
                    (ret, params)
                }
                Err(_) => ("?".to_string(), Vec::new()),
            };
            if mi > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "\n        {{\"name\": {}, \"returnType\": {}, \"params\": [",
                esc(&mname), esc(&ret_type)
            ));
            for (pi, pt) in params.iter().enumerate() {
                if pi > 0 {
                    s.push_str(", ");
                }
                s.push_str(&esc(pt));
            }
            s.push_str("]}");
        }
        if !method_rows.is_empty() {
            s.push_str("\n      ");
        }
        s.push_str("]\n");

        s.push_str("    }");
    }
    s.push_str("\n  ]\n");
    s.push_str("}\n");
    Ok(s)
}
