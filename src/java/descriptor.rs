//! Field and method descriptor parsing (JVMS 4.3).

use crate::error::{Error, Result};

/// Parse a field descriptor (e.g. `[[I`, `Ljava/lang/String;`) into a
/// dotted Java type name (e.g. `int[][]`, `java.lang.String`).
pub fn parse_field_descriptor(desc: &str) -> Result<String> {
    let (ty, consumed) = parse_type(desc)?;
    if consumed != desc.len() {
        return Err(Error::InvalidClassFile(format!(
            "trailing characters in field descriptor '{desc}'"
        )));
    }
    Ok(ty)
}

/// Parse a method descriptor `(ID)V` into (parameter types, return type).
pub fn parse_method_descriptor(desc: &str) -> Result<(Vec<String>, String)> {
    let mut chars = desc.chars().peekable();
    if chars.next() != Some('(') {
        return Err(Error::InvalidClassFile(format!(
            "method descriptor must start with '(': '{desc}'"
        )));
    }
    let mut params = Vec::new();
    loop {
        match chars.peek() {
            Some(')') => {
                chars.next();
                break;
            }
            None => {
                return Err(Error::InvalidClassFile(format!(
                    "unterminated parameter list in '{desc}'"
                )))
            }
            Some(_) => {
                let rest: String = chars.clone().collect();
                let (ty, consumed) = parse_type(&rest)?;
                params.push(ty);
                for _ in 0..consumed {
                    chars.next();
                }
            }
        }
    }
    let ret_rest: String = chars.collect();
    let (ret, consumed) = parse_type(&ret_rest)?;
    if consumed != ret_rest.len() {
        return Err(Error::InvalidClassFile(format!(
            "trailing characters in method descriptor '{desc}'"
        )));
    }
    Ok((params, ret))
}

/// Parse one type at the start of `s`; returns (type name, chars consumed).
fn parse_type(s: &str) -> Result<(String, usize)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(Error::InvalidClassFile("empty descriptor".into()));
    }
    let mut dims = 0;
    let mut i = 0;
    while bytes[i] == b'[' {
        dims += 1;
        i += 1;
        if i >= bytes.len() {
            return Err(Error::InvalidClassFile(format!("dangling '[' in '{s}'")));
        }
    }
    let base = match bytes[i] {
        b'B' => "byte".to_string(),
        b'C' => "char".to_string(),
        b'D' => "double".to_string(),
        b'F' => "float".to_string(),
        b'I' => "int".to_string(),
        b'J' => "long".to_string(),
        b'S' => "short".to_string(),
        b'Z' => "boolean".to_string(),
        b'V' => "void".to_string(),
        b'L' => {
            let end = s[i..]
                .find(';')
                .ok_or_else(|| Error::InvalidClassFile(format!("unterminated 'L' type in '{s}'")))?;
            crate::java::classfile::to_dotted(&s[i + 1..i + end])
        }
        other => {
            return Err(Error::InvalidClassFile(format!(
                "unknown descriptor char '{}' in '{s}'",
                other as char
            )))
        }
    };
    i += if bytes[i] == b'L' {
        s[i..].find(';').unwrap() + 1
    } else {
        1
    };
    Ok((format!("{}{}", base, "[]".repeat(dims)), i))
}

/// Collect type-parameter NAMES from `<Name:Bound(:Bound)*...>` and return
/// the text after the closing `>`. Returns `(names, rest)`; if `s` does not
/// start with `<`, returns an empty list.
fn parse_type_param_names(s: &str) -> Result<(Vec<String>, &str)> {
    let Some(mut rest) = s.strip_prefix('<') else {
        return Ok((Vec::new(), s));
    };
    let mut names = Vec::new();
    loop {
        if let Some(r) = rest.strip_prefix('>') {
            return Ok((names, r));
        }
        if let Some(r) = rest.strip_prefix(':') {
            // Additional bound of the previous type parameter.
            rest = skip_bound(r);
            continue;
        }
        match rest.find(':') {
            Some(colon) => {
                names.push(rest[..colon].to_string());
                rest = skip_bound(&rest[colon + 1..]);
            }
            None => {
                return Err(Error::InvalidClassFile(format!(
                    "unterminated type-parameter list in '{s}'"
                )))
            }
        }
    }
}

/// Walk past one bound (class or interface bound) starting at `rest`,
/// returning the text after its terminating `;` (or at a bare `>` ending the
/// type-parameter list). Bounds may contain nested `<>`, so `;` only
/// terminates at depth 0.
fn skip_bound(rest: &str) -> &str {
    let bytes = rest.as_bytes();
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => {
                if depth == 0 {
                    return &rest[i..];
                }
                depth -= 1;
            }
            b';' if depth == 0 => return &rest[i + 1..],
            _ => {}
        }
        i += 1;
    }
    &rest[i..]
}

/// Parse a type signature (JVMS 4.7.9.1): one `TypeSignature` — reference
/// types with generics (`Ljava/util/List<Ljava/lang/String;>;`), type
/// variables (`TT;`), arrays (`[TT;`), or primitives (`I`).
/// Returns (rendered Java type, chars consumed).
fn parse_type_signature_at(s: &str) -> Result<(String, usize)> {
    match s.as_bytes().first() {
        None => Err(Error::InvalidClassFile("empty type signature".into())),
        Some(b'[') => {
            let (comp, n) = parse_type_signature_at(&s[1..])?;
            Ok((format!("{comp}[]"), n + 1))
        }
        Some(b'T') => {
            let end = s
                .find(';')
                .ok_or_else(|| Error::InvalidClassFile(format!("unterminated type variable in '{s}'")))?;
            Ok((s[1..end].to_string(), end + 1))
        }
        Some(b'L') => parse_class_type_sig(s),
        _ => parse_type(s), // primitives — no generics possible
    }
}

/// `ClassTypeSignature`: `L pkg/name<args>.Inner<args> ;`
fn parse_class_type_sig(s: &str) -> Result<(String, usize)> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'L') {
        return Err(Error::InvalidClassFile(format!(
            "class type signature must start with 'L': '{s}'"
        )));
    }
    let mut i = 1;
    let mut out = String::new();
    loop {
        let start = i;
        while i < bytes.len() && !matches!(bytes[i], b'<' | b'>' | b';' | b'.' | b'/') {
            i += 1;
        }
        if i >= bytes.len() {
            return Err(Error::InvalidClassFile(format!(
                "unterminated class type signature '{s}'"
            )));
        }
        out.push_str(&s[start..i]);
        match bytes[i] {
            b'/' | b'.' => {
                // '/' separates package segments, '.' an inner class — both
                // render as '.'.
                out.push('.');
                i += 1;
            }
            b'<' => {
                let (args, n) = parse_type_arguments(&s[i..])?;
                out.push_str(&args);
                i += n;
            }
            _ => return Ok((out, i + 1)), // b';'
        }
    }
}

/// `TypeArguments`: `< TypeArgument+ >` → `<T, ? extends Number>`
fn parse_type_arguments(s: &str) -> Result<(String, usize)> {
    let mut i = 1; // skip '<'
    let mut args = Vec::new();
    loop {
        match s.as_bytes().get(i) {
            Some(b'>') => {
                i += 1;
                break;
            }
            None | Some(&b';') => {
                return Err(Error::InvalidClassFile(format!(
                    "unterminated type arguments in '{s}'"
                )))
            }
            _ => {}
        }
        let (arg, n) = parse_type_argument(&s[i..])?;
        args.push(arg);
        i += n;
    }
    Ok((format!("<{}>", args.join(", ")), i))
}

/// `TypeArgument`: `[+|-] TypeSignature | *`
fn parse_type_argument(s: &str) -> Result<(String, usize)> {
    match s.as_bytes().first() {
        Some(b'*') => Ok(("*".into(), 1)),
        Some(b'+') => {
            let (t, n) = parse_type_signature_at(&s[1..])?;
            Ok((format!("? extends {t}"), n + 1))
        }
        Some(b'-') => {
            let (t, n) = parse_type_signature_at(&s[1..])?;
            Ok((format!("? super {t}"), n + 1))
        }
        _ => parse_type_signature_at(s),
    }
}

/// Parse a full field signature (JVMS 4.7.9.1) into rendered Java type text.
pub fn parse_type_signature(sig: &str) -> Result<String> {
    let (ty, consumed) = parse_type_signature_at(sig)?;
    if consumed != sig.len() {
        return Err(Error::InvalidClassFile(format!(
            "trailing characters in field signature '{sig}'"
        )));
    }
    Ok(ty)
}

/// Parse a method signature (JVMS 4.7.9.1):
/// `[TypeParams] ( ParamType* ) ReturnType ThrowsSig*`
/// Returns (method type-parameter names, parameter types, return type).
/// Throws signatures are ignored (the `Exceptions` attribute covers them).
pub fn parse_method_signature(sig: &str) -> Result<(Vec<String>, Vec<String>, String)> {
    let (type_params, rest) = parse_type_param_names(sig)?;
    let rest = rest.strip_prefix('(').ok_or_else(|| {
        Error::InvalidClassFile(format!("method signature must have '(': '{sig}'"))
    })?;
    let mut params = Vec::new();
    let mut rest = rest;
    loop {
        if let Some(r) = rest.strip_prefix(')') {
            rest = r;
            break;
        }
        if rest.is_empty() {
            return Err(Error::InvalidClassFile(format!(
                "unterminated parameter list in '{sig}'"
            )));
        }
        let (ty, consumed) = parse_type_signature_at(rest)?;
        params.push(ty);
        rest = &rest[consumed..];
    }
    // Return type; anything after it (`^Throws...`) is ignored.
    let (ret, _) = parse_type_signature_at(rest)?;
    Ok((type_params, params, ret))
}

/// Generic `Signature` attribute parsing (JVMS 4.7.9.1) — class type
/// parameters only (e.g. `<T:Ljava/lang/Object;>Ljava/lang/Object;`).
/// Returns the declared type-parameter names (e.g. `["T", "U"]`), or an
/// empty vector when the class has no formal type parameters.
pub fn parse_class_type_params(signature: &str) -> Vec<String> {
    parse_type_param_names(signature)
        .map(|(names, _)| names)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_descriptors() {
        assert_eq!(parse_field_descriptor("I").unwrap(), "int");
        assert_eq!(parse_field_descriptor("Z").unwrap(), "boolean");
        assert_eq!(parse_field_descriptor("J").unwrap(), "long");
        assert_eq!(parse_field_descriptor("Ljava/lang/String;").unwrap(), "java.lang.String");
        assert_eq!(parse_field_descriptor("[[I").unwrap(), "int[][]");
        assert_eq!(
            parse_field_descriptor("[Ljava/lang/Object;").unwrap(),
            "java.lang.Object[]"
        );
    }

    #[test]
    fn method_descriptors() {
        let (params, ret) = parse_method_descriptor("(ID)V").unwrap();
        assert_eq!(params, vec!["int", "double"]);
        assert_eq!(ret, "void");

        let (params, ret) = parse_method_descriptor("([Ljava/lang/String;I)[I").unwrap();
        assert_eq!(params, vec!["java.lang.String[]", "int"]);
        assert_eq!(ret, "int[]");
    }

    #[test]
    fn bad_descriptors() {
        assert!(parse_field_descriptor("X").is_err());
        assert!(parse_field_descriptor("[").is_err());
        assert!(parse_method_descriptor("I)V").is_err());
        assert!(parse_method_descriptor("(I").is_err());
    }

    #[test]
    fn class_type_params() {
        assert_eq!(parse_class_type_params("<T:Ljava/lang/Object;>Ljava/lang/Object;"), vec!["T"]);
        assert_eq!(
            parse_class_type_params("<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;"),
            vec!["K", "V"]
        );
        assert_eq!(parse_class_type_params("Ljava/lang/Object;"), Vec::<String>::new());
    }
}
