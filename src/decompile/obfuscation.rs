//! Obfuscation detection — scan an assembly for common obfuscation indicators
//! and emit warnings.

use crate::error::Result;
use crate::metadata::reader::Reader;
use crate::metadata::tables::tbl;

/// A single obfuscation warning.
#[derive(Debug, Clone)]
pub struct ObfuscationWarning {
    pub severity: &'static str, // "high", "medium", "low"
    pub category: &'static str, // "names", "strings", "control-flow", etc.
    pub message: String,
}

/// Scan the assembly for obfuscation indicators.
pub fn detect_obfuscation(reader: &Reader<'_>) -> Result<Vec<ObfuscationWarning>> {
    let mut warnings = Vec::new();

    // 1. Check for non-printable / unusual type and method names.
    let mut unprintable_names = 0;
    let mut very_long_names = 0;
    let mut total_types = 0;
    let mut total_methods = 0;
    let mut max_switch_cases = 0;

    let type_defs = reader.tables.get(tbl::TYPEDEF);
    for (i, row) in type_defs.iter().enumerate() {
        let name = reader.type_def_name(row);
        if name == "<Module>" {
            continue;
        }
        total_types += 1;

        // Check for non-printable characters (excluding common compiler-generated
        // markers like <, >, _, etc.)
        if has_unprintable_name(&name) {
            unprintable_names += 1;
        }

        // Check for very long names (> 100 chars)
        if name.len() > 100 {
            very_long_names += 1;
        }

        // Check methods in this type
        let row_idx = (i + 1) as u32;
        for mi in reader.type_method_rows(row_idx) {
            let m = &reader.tables.get(tbl::METHODDEF)[mi];
            let mname = reader.method_name(m);
            total_methods += 1;

            if has_unprintable_name(&mname) {
                unprintable_names += 1;
            }
            if mname.len() > 100 {
                very_long_names += 1;
            }

            // Check for large switch statements (control-flow flattening indicator)
            if let Some(switch_cases) = count_switch_cases(reader, m) {
                if switch_cases > max_switch_cases {
                    max_switch_cases = switch_cases;
                }
            }
        }
    }

    // 2. Non-printable names — high severity
    if unprintable_names > 0 {
        warnings.push(ObfuscationWarning {
            severity: "high",
            category: "names",
            message: format!(
                "{unprintable_names} type/method name(s) contain non-printable or unusual characters — likely obfuscated"
            ),
        });
    }

    // 3. Very long names — medium severity
    if very_long_names > 0 {
        warnings.push(ObfuscationWarning {
            severity: "medium",
            category: "names",
            message: format!(
                "{very_long_names} type/method name(s) exceed 100 characters — possible obfuscation"
            ),
        });
    }

    // 4. Large switch statements — control-flow flattening indicator
    if max_switch_cases > 50 {
        warnings.push(ObfuscationWarning {
            severity: "high",
            category: "control-flow",
            message: format!(
                "Method with {max_switch_cases} switch cases detected — possible control-flow flattening"
            ),
        });
    } else if max_switch_cases > 20 {
        warnings.push(ObfuscationWarning {
            severity: "low",
            category: "control-flow",
            message: format!(
                "Method with {max_switch_cases} switch cases — large but may be legitimate"
            ),
        });
    }

    // 5. Check for string encryption — look for methods that call a single
    // decrypt-like method with many string literal arguments.
    let string_decrypt = detect_string_encryption(reader);
    if let Some(count) = string_decrypt {
        warnings.push(ObfuscationWarning {
            severity: "high",
            category: "strings",
            message: format!(
                "Possible string encryption detected — {count} calls to a single method with string literal arguments"
            ),
        });
    }

    // 6. Summary
    if warnings.is_empty() {
        warnings.push(ObfuscationWarning {
            severity: "info",
            category: "summary",
            message: format!(
                "No obfuscation indicators found ({total_types} types, {total_methods} methods scanned)"
            ),
        });
    } else {
        warnings.push(ObfuscationWarning {
            severity: "info",
            category: "summary",
            message: format!(
                "Scanned {total_types} types and {total_methods} methods, found {} warning(s)",
                warnings.len()
            ),
        });
    }

    Ok(warnings)
}

/// Check if a name contains non-printable or unusual characters.
/// Compiler-generated names like `<Name>b__0` or `<>c__DisplayClass` are
/// excluded (they use `<`, `>`, `_` which are valid compiler markers).
fn has_unprintable_name(name: &str) -> bool {
    for c in name.chars() {
        // Allow: alphanumeric, _, <, >, $, ., backtick
        if c.is_alphanumeric() || c == '_' || c == '<' || c == '>' || c == '$' || c == '.' || c == '`' {
            continue;
        }
        // Allow common operators
        if c == '-' || c == '+' || c == '=' || c == '!' || c == '~' || c == '|' || c == '&' || c == '^' {
            continue;
        }
        // Non-printable or unusual
        if !c.is_ascii_graphic() && c != ' ' {
            return true;
        }
        // Unusual printable chars (e.g., unicode confusables)
        if (c as u32) > 127 {
            return true;
        }
    }
    false
}

/// Count the number of cases in a switch instruction within a method body.
/// Returns None if the method has no switch.
fn count_switch_cases(reader: &Reader<'_>, m: &crate::metadata::tables::Row) -> Option<usize> {
    let rva = reader.method_rva(m);
    let body = reader.method_body(rva).ok()??;
    let instructions = crate::cil::decoder::decode(&body.code).ok()?;
    for ins in &instructions {
        if ins.name == "switch" {
            if let crate::cil::decoder::Operand::Switch(targets) = &ins.operand {
                return Some(targets.len());
            }
        }
    }
    None
}

/// Detect possible string encryption by looking for a method that is called
/// many times with a string literal as an argument.
/// Returns the count of calls if suspicious, None otherwise.
fn detect_string_encryption(reader: &Reader<'_>) -> Option<usize> {
    // Count calls to each method token from ldstr + call patterns.
    let mut call_counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();

    let type_defs = reader.tables.get(tbl::TYPEDEF);
    for (i, row) in type_defs.iter().enumerate() {
        let name = reader.type_def_name(row);
        if name == "<Module>" {
            continue;
        }
        let row_idx = (i + 1) as u32;
        for mi in reader.type_method_rows(row_idx) {
            let m = &reader.tables.get(tbl::METHODDEF)[mi];
            let rva = reader.method_rva(m);
            if let Some(body) = reader.method_body(rva).ok().flatten() {
                if let Ok(instructions) = crate::cil::decoder::decode(&body.code) {
                    let mut prev_was_ldstr = false;
                    for ins in &instructions {
                        if ins.name == "ldstr" {
                            prev_was_ldstr = true;
                        } else if prev_was_ldstr && (ins.name == "call" || ins.name == "callvirt") {
                            if let crate::cil::decoder::Operand::Token(tok) = &ins.operand {
                                *call_counts.entry(*tok).or_insert(0) += 1;
                            }
                            prev_was_ldstr = false;
                        } else {
                            prev_was_ldstr = false;
                        }
                    }
                }
            }
        }
    }

    // If any single method is called with string literals more than 10 times,
    // it's suspicious for string encryption.
    for (_, count) in &call_counts {
        if *count > 10 {
            return Some(*count);
        }
    }
    None
}

/// Format warnings for CLI output.
pub fn format_warnings(warnings: &[ObfuscationWarning]) -> String {
    let mut s = String::new();
    s.push_str("Obfuscation detection report\n");
    s.push_str("============================\n\n");
    for w in warnings {
        let icon = match w.severity {
            "high" => "[!]",
            "medium" => "[~]",
            "low" => "[*]",
            "info" => "[i]",
            _ => "[?]",
        };
        s.push_str(&format!("{icon} ({}) {}: {}\n", w.severity, w.category, w.message));
    }
    s
}
