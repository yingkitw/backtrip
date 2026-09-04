//! Structural verification — check that decompiled output is consistent
//! with the metadata (all types, methods, and fields appear in the output).

use crate::error::Result;
use crate::metadata::reader::Reader;
use crate::metadata::tables::tbl;

/// A verification result item.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub status: &'static str, // "ok", "missing", "extra"
    pub category: &'static str, // "type", "method", "field"
    pub message: String,
}

/// Verify that the decompiled source contains all types, methods, and fields
/// from the metadata.
pub fn verify(reader: &Reader<'_>, types: &[crate::decompile::DecompiledType]) -> Result<Vec<VerifyResult>> {
    let mut results = Vec::new();

    // Combine all decompiled source into one string for searching.
    let all_source: String = types.iter().map(|t| t.source.as_str()).collect();

    let mut total_types = 0;
    let mut total_methods = 0;
    let mut total_fields = 0;
    let mut missing_types = 0;
    let mut missing_methods = 0;
    let mut missing_fields = 0;

    let type_defs = reader.tables.get(tbl::TYPEDEF);
    for (i, row) in type_defs.iter().enumerate() {
        let name = reader.type_def_name(row);
        if name == "<Module>" {
            continue;
        }
        total_types += 1;

        // Check that the type name appears in the source.
        // For display class names, check the cleaned name.
        let clean_name = crate::decompile::csharp::clean_display_class_name_pub(&name);
        if !all_source.contains(&clean_name) && !all_source.contains(&name) {
            results.push(VerifyResult {
                status: "missing",
                category: "type",
                message: format!("Type '{clean_name}' not found in decompiled output"),
            });
            missing_types += 1;
        }

        // Check methods.
        let row_idx = (i + 1) as u32;
        for mi in reader.type_method_rows(row_idx) {
            let m = &reader.tables.get(tbl::METHODDEF)[mi];
            let mname = reader.method_name(m);
            // Skip property getter/setter and event add/remove — they're
            // rendered as part of properties/events, not standalone methods.
            if mname.starts_with("get_") || mname.starts_with("set_")
                || mname.starts_with("add_") || mname.starts_with("remove_") {
                continue;
            }
            // Skip delegate methods (Invoke, BeginInvoke, EndInvoke) —
            // delegates are rendered as `delegate ...` declarations.
            if mname == "Invoke" || mname == "BeginInvoke" || mname == "EndInvoke" {
                continue;
            }
            // Skip constructors — they're rendered as `TypeName(...)`.
            if mname == ".ctor" || mname == ".cctor" {
                continue;
            }
            // Skip explicit interface implementations — they're rendered
            // with the interface-qualified name.
            if mname.contains('.') {
                continue;
            }
            total_methods += 1;

            let clean_mname = crate::decompile::csharp::clean_display_class_name_pub(&mname);
            if !all_source.contains(&clean_mname) && !all_source.contains(&mname) {
                results.push(VerifyResult {
                    status: "missing",
                    category: "method",
                    message: format!("Method '{clean_mname}' not found in decompiled output"),
                });
                missing_methods += 1;
            }
        }

        // Check fields.
        for fi in reader.type_field_rows(row_idx) {
            let f = &reader.tables.get(tbl::FIELD)[fi];
            let fname = reader.field_name(f);
            // Skip backing fields — they're rendered as part of properties.
            if fname.contains("k__BackingField") {
                continue;
            }
            // Skip enum backing field `value__`.
            if fname == "value__" {
                continue;
            }
            total_fields += 1;

            let clean_fname = crate::decompile::csharp::clean_field_name_pub(&fname);
            if !all_source.contains(&clean_fname) && !all_source.contains(&fname) {
                results.push(VerifyResult {
                    status: "missing",
                    category: "field",
                    message: format!("Field '{clean_fname}' not found in decompiled output"),
                });
                missing_fields += 1;
            }
        }
    }

    // Summary
    let ok = missing_types == 0 && missing_methods == 0 && missing_fields == 0;
    results.push(VerifyResult {
        status: if ok { "ok" } else { "missing" },
        category: "summary",
        message: format!(
            "Verified {total_types} types, {total_methods} methods, {total_fields} fields — {} missing",
            missing_types + missing_methods + missing_fields
        ),
    });

    Ok(results)
}

/// Format verification results for CLI output.
pub fn format_results(results: &[VerifyResult]) -> String {
    let mut s = String::new();
    s.push_str("Verification report\n");
    s.push_str("===================\n\n");
    for r in results {
        let icon = match r.status {
            "ok" => "[+]",
            "missing" => "[-]",
            "extra" => "[?]",
            _ => "[*]",
        };
        s.push_str(&format!("{icon} ({}) {}: {}\n", r.status, r.category, r.message));
    }
    s
}
