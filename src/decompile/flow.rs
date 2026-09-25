//! Language-neutral control-flow restructuring and expression helpers shared
//! by the C# and Java decompilers.
//!
//! Both back ends emit a flat, goto-based statement list (`Vec<String>`) with
//! `Label_NNNN:` labels and `goto Label_NNNN;` jumps. These passes fold that
//! list into structured source: if/else blocks, while/do-while/for loops and
//! switch statements. Anything not recognized stays as-is; `cleanup_goto_labels`
//! comments out the leftovers (Java has no `goto`, so this step is mandatory
//! there for valid output).

/// Canonical label name for a bytecode offset.
pub fn label_name(offset: usize) -> String {
    format!("Label_{offset:04X}")
}

/// Indent lines `out[start..end]` by 4 spaces, skipping empty lines.
pub fn indent_range(out: &mut [String], start: usize, end: usize) {
    for line in out[start..end].iter_mut() {
        if !line.trim().is_empty() {
            *line = format!("    {line}");
        }
    }
}

pub fn prec(op: &str) -> u8 {
    match op {
        "*" | "/" | "%" => 7,
        "+" | "-" => 6,
        "<<" | ">>" | ">>>" => 5,
        "<" | "<=" | ">" | ">=" | "==" | "!=" => 4,
        "&" => 3,
        "^" => 2,
        "|" => 1,
        "&&" | "||" => 0,
        _ => 0,
    }
}

/// Strip outer parentheses from an expression if present (and balanced).
pub fn strip_outer_parens(s: &str) -> &str {
    let t = s.trim();
    if t.starts_with('(') && t.ends_with(')') {
        let inner = &t[1..t.len() - 1];
        let mut depth = 0;
        for c in inner.chars() {
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
        }
        if depth == 0 {
            return inner.trim();
        }
    }
    t
}

/// Get the top-level operator of an expression (for precedence comparison).
pub fn top_op(s: &str) -> Option<&str> {
    let t = strip_outer_parens(s);
    let mut depth = 0;
    let mut best: Option<(&str, usize)> = None;
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ if depth == 0 => {
                let two = if i + 1 < bytes.len() {
                    std::str::from_utf8(&bytes[i..i + 2]).ok()
                } else {
                    None
                };
                if let Some(two) = two
                    && matches!(
                        two,
                        ">>>" | "<<" | ">>" | "<=" | ">=" | "==" | "!=" | "&&" | "||"
                    ) {
                        let p = prec(two) as usize;
                        if best.map(|(_, bp)| p <= bp).unwrap_or(true) {
                            best = Some((two, p));
                        }
                        i += 2;
                        continue;
                    }
                let one = std::str::from_utf8(&bytes[i..i + 1]).ok();
                if let Some(one) = one
                    && matches!(
                        one,
                        "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "<" | ">"
                    ) {
                        let p = prec(one) as usize;
                        if best.map(|(_, bp)| p <= bp).unwrap_or(true) {
                            best = Some((one, p));
                        }
                    }
            }
            _ => {}
        }
        i += 1;
    }
    best.map(|(op, _)| op)
}

/// Pop two operands and push `a op b`, adding parens only where the
/// operand's top-level operator binds looser than (or, on the right,
/// equal to) `op` — and stripping them where they are redundant.
pub fn binop(stack: &mut Vec<String>, op: &str) {
    let b = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let a = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let my_prec = prec(op);
    // Left operand: same precedence is fine (binary ops are left-associative).
    let a_use = match top_op(&a) {
        Some(o) if prec(o) >= my_prec => strip_outer_parens(&a).to_string(),
        Some(_) => format!("({})", strip_outer_parens(&a)),
        None => strip_outer_parens(&a).to_string(),
    };
    // Right operand: equal precedence needs parens (`a - (b - c)` != `a - b - c`).
    let b_use = match top_op(&b) {
        Some(o) if prec(o) > my_prec => strip_outer_parens(&b).to_string(),
        Some(_) => format!("({})", strip_outer_parens(&b)),
        None => strip_outer_parens(&b).to_string(),
    };
    stack.push(format!("{a_use} {op} {b_use}"));
}

pub fn unop(stack: &mut Vec<String>, op: &str) {
    let a = stack.pop().unwrap_or_else(|| "/*?*/".into());
    stack.push(format!("{op}{a}"));
}

pub fn conv(stack: &mut Vec<String>, ty: &str) {
    let a = stack.pop().unwrap_or_else(|| "/*?*/".into());
    let a_stripped = strip_outer_parens(&a);
    stack.push(format!("({ty})({a_stripped})"));
}

/// Wrap an operand in parens if its top-level operator binds looser than the
/// operator it is fed to.
pub fn paren_if_needed(e: &str, op: &str) -> String {
    let s = strip_outer_parens(e);
    if top_op(s).map(|o| prec(o) < prec(op)).unwrap_or(false) {
        format!("({s})")
    } else {
        s.to_string()
    }
}

/// Strip ALL redundant outer parentheses: `((x))` → `x`. The `(!x)`-strip
/// path of `negate_cond` can double-wrap when the inner expression is
/// already parenthesized.
pub fn unwrap_parens(s: &str) -> &str {
    let mut cur = s.trim();
    loop {
        let next = strip_outer_parens(cur);
        if next == cur {
            return cur;
        }
        cur = next;
    }
}

pub fn negate_cond(cond: &str) -> String {
    // If cond is `(!x)` or `!(x)`, strip the negation.
    let trimmed = cond.trim();
    if trimmed.starts_with("(!") && trimmed.ends_with(')') {
        let inner = &trimmed[2..trimmed.len() - 1];
        return format!("({inner})");
    }
    if trimmed.starts_with("!(") && trimmed.ends_with(')') {
        let inner = &trimmed[2..trimmed.len() - 1];
        return format!("({inner})");
    }
    let ops = [
        (">=", "<"),
        ("<=", ">"),
        (">", "<="),
        ("<", ">="),
        ("==", "!="),
        ("!=", "=="),
    ];
    for (a, b) in ops {
        if cond.contains(a) {
            return cond.replacen(a, b, 1);
        }
    }
    format!("!{cond}")
}

/// Fold `if (cond) goto Label;` + block + `Label:` into `if (!cond) { block }`.
pub fn restructure_if_else(out: &mut [String]) {
    let mut i = 0;
    while i < out.len() {
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.starts_with("if (") || !trimmed.contains(") goto Label_") {
            i += 1;
            continue;
        }
        let label = match trimmed.rsplit("goto ").next() {
            Some(s) => s.trim().trim_end_matches(';'),
            None => {
                i += 1;
                continue;
            }
        };
        let cond_start = trimmed.find("if (").map(|p| p + 4).unwrap_or(0);
        let cond_end = match trimmed[cond_start..].find(") goto") {
            Some(p) => cond_start + p,
            None => {
                i += 1;
                continue;
            }
        };
        let cond = format!("({})", &trimmed[cond_start..cond_end]);

        let label_pattern = format!("Label_{}:", label.trim_start_matches("Label_"));
        let label_idx = out[i + 1..].iter().position(|l| l.trim() == label_pattern);
        let label_idx = match label_idx {
            Some(p) => i + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };

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

        let block_has_label = out[i + 1..label_idx].iter().any(|l| {
            let t = l.trim();
            t.starts_with("Label_") && t.ends_with(':')
        });
        if block_has_label {
            i += 1;
            continue;
        }

        let label_name = label.trim();
        let label_referenced_elsewhere = out.iter().enumerate().any(|(k, l)| {
            k != i
                && k != label_idx
                && l
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .any(|tok| tok == label_name)
        });
        if label_referenced_elsewhere {
            i += 1;
            continue;
        }

        let has_else = block_last.starts_with("goto ");
        let else_label = if has_else {
            block_last.trim_start_matches("goto ").trim_end_matches(';').to_string()
        } else {
            String::new()
        };

        let neg_cond = negate_cond(&cond);
        let neg_cond = unwrap_parens(&neg_cond);
        out[i] = format!("        if ({neg_cond}) {{");

        indent_range(out, i + 1, label_idx);

        if has_else {
            let else_pattern = format!("Label_{}:", else_label.trim_start_matches("Label_"));
            let else_idx = out[label_idx + 1..]
                .iter()
                .position(|l| l.trim() == else_pattern)
                .map(|p| label_idx + 1 + p);
            if let Some(ei) = else_idx {
                out[label_idx] = "        } else {".to_string();
                indent_range(out, label_idx + 1, ei);
                out[ei] = "        }".to_string();
                i = ei + 1;
                continue;
            }
        }

        out[label_idx] = "        }".to_string();
        i = label_idx + 1;
    }
}

/// Fold back-edge patterns into `while (cond) { ... }` loops.
pub fn restructure_while_loops(out: &mut [String]) {
    let mut i = 0;
    while i < out.len() {
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.starts_with("goto Label_") {
            i += 1;
            continue;
        }
        let header_label = trimmed.trim_start_matches("goto ").trim_end_matches(';');

        let header_pattern = format!("Label_{}:", header_label.trim_start_matches("Label_"));
        let header_idx = match out[i + 1..].iter().position(|l| l.trim() == header_pattern) {
            Some(p) => i + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };

        if header_idx + 1 >= out.len() {
            i += 1;
            continue;
        }
        let cond_line = out[header_idx + 1].trim().to_string();
        if !cond_line.starts_with("if (") || !cond_line.contains(") goto Label_") {
            i += 1;
            continue;
        }

        let back_label = match cond_line.rsplit("goto ").next() {
            Some(s) => s.trim().trim_end_matches(';'),
            None => {
                i += 1;
                continue;
            }
        };

        let back_pattern = format!("Label_{}:", back_label.trim_start_matches("Label_"));
        let body_start = match out[i + 1..header_idx]
            .iter()
            .position(|l| l.trim() == back_pattern)
        {
            Some(p) => i + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };

        let cond_start = cond_line.find("if (").map(|p| p + 4).unwrap_or(0);
        let cond_end = match cond_line[cond_start..].find(") goto") {
            Some(p) => cond_start + p,
            None => {
                i += 1;
                continue;
            }
        };
        let cond = format!("({})", &cond_line[cond_start..cond_end]);

        // The back-edge means "continue while true" — condition is NOT negated.
        out[i] = format!("        while {cond} {{");
        out[body_start] = String::new();

        indent_range(out, body_start + 1, header_idx);

        out[header_idx] = "        }".to_string();
        out[header_idx + 1] = String::new();

        i = header_idx + 2;
    }
}

/// Fold label + back-edge patterns into `do { ... } while (cond);`.
pub fn restructure_do_while_loops(out: &mut [String]) {
    let mut i = 0;
    while i < out.len() {
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.starts_with("Label_") || !trimmed.ends_with(':') {
            i += 1;
            continue;
        }
        let label_name = trimmed.trim_end_matches(':');

        // Exclude while-loop headers (preceded by a goto to a different label).
        if i > 0 {
            let prev = out[i - 1].trim();
            if prev.starts_with("goto Label_")
                && !prev.contains(&format!("goto {};", label_name))
            {
                i += 1;
                continue;
            }
        }

        let back_pattern = format!("goto {};", label_name);
        let back_idx = out[i + 1..]
            .iter()
            .position(|l| l.trim().contains(&back_pattern) && l.trim().starts_with("if ("));
        let back_idx = match back_idx {
            Some(p) => i + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };

        let back_line = out[back_idx].trim().to_string();
        if !back_line.starts_with("if (") || !back_line.ends_with(';') {
            i += 1;
            continue;
        }

        let cond_start = back_line.find("if (").map(|p| p + 4).unwrap_or(0);
        let cond_end = match back_line[cond_start..].find(") goto") {
            Some(p) => cond_start + p,
            None => {
                i += 1;
                continue;
            }
        };
        let cond = format!("({})", &back_line[cond_start..cond_end]);

        out[i] = "        do {".to_string();
        indent_range(out, i + 1, back_idx);
        out[back_idx] = format!("        }} while {cond};");

        i = back_idx + 1;
    }
}

/// Fold javac-style while loops. javac (unlike Roslyn) emits the condition
/// check directly after the init code — no initial `goto`:
///   Label_XXXX:                  (condition header)
///   if (cond) goto Label_YYYY;   (jump to loop exit)
///   ... body ...
///   goto Label_XXXX;             (back-edge)
///   Label_YYYY:                  (after loop)
/// Transforms to `while (!(cond)) { body }`. Must run before
/// `restructure_if_else`, which would otherwise consume the branch.
pub fn restructure_while_loops_javac(out: &mut [String]) {
    let mut i = 0;
    while i + 1 < out.len() {
        let header = out[i].trim();
        if !header.starts_with("Label_") || !header.ends_with(':') {
            i += 1;
            continue;
        }
        let header_label = header.trim_end_matches(':').to_string();

        let cond_line = out[i + 1].trim().to_string();
        if !cond_line.starts_with("if (") || !cond_line.contains(") goto Label_") {
            i += 1;
            continue;
        }
        let exit_label = match cond_line.rsplit("goto ").next() {
            Some(s) => s.trim().trim_end_matches(';').to_string(),
            None => {
                i += 1;
                continue;
            }
        };
        if exit_label == header_label {
            // `goto` back to the same label is a do-while, not a while.
            i += 1;
            continue;
        }

        let exit_pattern = format!("Label_{}:", exit_label.trim_start_matches("Label_"));
        let exit_idx = match out[i + 2..].iter().position(|l| l.trim() == exit_pattern) {
            Some(p) => i + 2 + p,
            None => {
                i += 1;
                continue;
            }
        };
        if exit_idx <= i + 2 {
            i += 1;
            continue;
        }

        // The block must end with the back-edge `goto Label_XXXX;`.
        let block_last = out[exit_idx - 1].trim().to_string();
        if block_last != format!("goto {header_label};") {
            i += 1;
            continue;
        }

        // Extract the exit condition; the loop continues while it does not hold.
        let cond_start = cond_line.find("if (").map(|p| p + 4).unwrap_or(0);
        let cond_end = match cond_line[cond_start..].find(") goto") {
            Some(p) => cond_start + p,
            None => {
                i += 1;
                continue;
            }
        };
        let neg_cond = negate_cond(&format!("({})", &cond_line[cond_start..cond_end]));
        // negate_cond can double-wrap — emit a single paren level.
        let cond_s = unwrap_parens(&neg_cond);

        out[i] = format!("        while ({cond_s}) {{");
        out[i + 1] = String::new();
        // Indent the body; drop the back-edge line.
        out[exit_idx - 1] = String::new();
        indent_range(out, i + 2, exit_idx - 1);
        out[exit_idx] = "        }".to_string();
        i = exit_idx + 1;
    }
}

/// Fold `init; while (cond) { body; incr; }` into `for (init; cond; incr)`.
pub fn restructure_for_loops(out: &mut [String]) {
    let mut i = 0;
    while i < out.len() {
        let line = &out[i];
        let trimmed = line.trim();
        if !trimmed.starts_with("while (") || !trimmed.ends_with('{') {
            i += 1;
            continue;
        }

        let cond_inner = trimmed.trim_start_matches("while (").trim_end_matches(") {");
        let loop_var = match cond_inner.split_whitespace().next() {
            Some(v) => v.trim(),
            None => {
                i += 1;
                continue;
            }
        };

        // The previous non-empty line is the init: `V = <init>;` or
        // `var V = <init>;` (Java locals need a declaration).
        let init_idx = if i > 0 {
            (0..i).rev().find(|&j| !out[j].trim().is_empty())
        } else {
            None
        };
        let init_idx = match init_idx {
            Some(idx) => idx,
            None => {
                i += 1;
                continue;
            }
        };
        let init_line = out[init_idx].trim().to_string();
        let var_decl = init_line.starts_with("var ");
        let plain_init = init_line.trim_start_matches("var ").to_string();
        if !plain_init.starts_with(&format!("{loop_var} = ")) || !plain_init.ends_with(';') {
            i += 1;
            continue;
        }
        let init_expr = &plain_init[format!("{loop_var} = ").len()..].trim_end_matches(';');

        let close_idx = match out[i + 1..].iter().position(|l| l.trim() == "}") {
            Some(p) => i + 1 + p,
            None => {
                i += 1;
                continue;
            }
        };

        let incr_idx = (i + 1..close_idx).rev().find(|&j| !out[j].trim().is_empty());
        let incr_idx = match incr_idx {
            Some(idx) => idx,
            None => {
                i += 1;
                continue;
            }
        };
        let incr_line = out[incr_idx].trim().to_string();
        // Increment: `V = <expr>;`, `V += D;` or `V -= D;`.
        let incr_expr = if incr_line.starts_with(&format!("{loop_var} += "))
            && incr_line.ends_with(';')
        {
            format!(
                "{loop_var} += {}",
                &incr_line[format!("{loop_var} += ").len()..].trim_end_matches(';')
            )
        } else if incr_line.starts_with(&format!("{loop_var} -= ")) && incr_line.ends_with(';') {
            format!(
                "{loop_var} -= {}",
                &incr_line[format!("{loop_var} -= ").len()..].trim_end_matches(';')
            )
        } else if incr_line.starts_with(&format!("{loop_var} = ")) && incr_line.ends_with(';') {
            format!(
                "{loop_var} = {}",
                &incr_line[format!("{loop_var} = ").len()..].trim_end_matches(';')
            )
        } else {
            i += 1;
            continue;
        };

        let cond_str = cond_inner;
        let init_part = if var_decl {
            format!("var {loop_var} = {init_expr}")
        } else {
            format!("{loop_var} = {init_expr}")
        };
        out[i] =
            format!("        for ({init_part}; {cond_str}; {incr_expr}) {{");
        out[init_idx] = String::new();
        out[incr_idx] = String::new();
        i = close_idx + 1;
    }
}

fn detect_switch_expr_pattern(
    cases: &[(i64, String)],
    label_bodies: &std::collections::HashMap<String, Vec<String>>,
) -> Option<String> {
    let mut common_label: Option<String> = None;
    let mut all_match = true;

    for (_, label) in cases {
        if let Some(body) = label_bodies.get(label) {
            if let Some(last) = body.last() {
                let last_trimmed = last.trim();
                if last_trimmed.starts_with("goto Label_") && last_trimmed.ends_with(';') {
                    let goto_label = last_trimmed
                        .strip_prefix("goto ")
                        .unwrap_or(last_trimmed)
                        .trim_end_matches(';')
                        .to_string();
                    if let Some(ref cl) = common_label {
                        if cl != &goto_label {
                            all_match = false;
                            break;
                        }
                    } else {
                        common_label = Some(goto_label);
                    }
                } else {
                    all_match = false;
                    break;
                }
            } else {
                all_match = false;
                break;
            }
        } else {
            all_match = false;
            break;
        }
    }

    if all_match {
        common_label
    } else {
        None
    }
}

/// Inline case bodies of a `switch` emitted as `case N: goto Label;` arms.
pub fn restructure_switch(out: &mut Vec<String>) {
    let mut i = 0;
    while i < out.len() {
        if !out[i].trim().starts_with("switch (") || !out[i].trim().ends_with(")") {
            i += 1;
            continue;
        }

        if i + 1 >= out.len() || out[i + 1].trim() != "{" {
            i += 1;
            continue;
        }

        let mut cases: Vec<(i64, String)> = Vec::new();
        let mut j = i + 2;
        while j < out.len() {
            let t = out[j].trim();
            if let Some(rest) = t.strip_prefix("case ")
                && let Some(goto_pos) = rest.find(": goto ") {
                    let case_num = &rest[..goto_pos];
                    let label_part = &rest[goto_pos + 7..].trim_end_matches(';');
                    if let Ok(n) = case_num.parse::<i64>() {
                        cases.push((n, label_part.to_string()));
                        j += 1;
                        continue;
                    }
                }
            break;
        }

        if cases.is_empty() {
            i += 1;
            continue;
        }

        if j >= out.len() || out[j].trim() != "}" {
            i += 1;
            continue;
        }
        let switch_close = j;

        let default_goto_idx = switch_close + 1;
        let default_label = if default_goto_idx < out.len() {
            let t = out[default_goto_idx].trim();
            if t.starts_with("goto Label_") {
                Some(t["goto ".len()..].trim_end_matches(';').to_string())
            } else {
                None
            }
        } else {
            None
        };

        let search_start = default_goto_idx + 1;
        let mut all_labels: Vec<(usize, String)> = Vec::new();
        for (k, line) in out.iter().enumerate().skip(search_start) {
            let t = line.trim();
            if t.starts_with("Label_") && t.ends_with(':') {
                all_labels.push((k, t.trim_end_matches(':').to_string()));
            }
        }

        let mut label_bodies: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for (idx, (line_idx, label)) in all_labels.iter().enumerate() {
            let body_start = line_idx + 1;
            let body_end = if idx + 1 < all_labels.len() {
                all_labels[idx + 1].0
            } else {
                let mut end = out.len();
                if let Some(k) = out[body_start..]
                    .iter()
                    .position(|l| l.trim() == "}")
                {
                    end = body_start + k;
                }
                end
            };
            let body: Vec<String> = out[body_start..body_end]
                .iter()
                .filter(|l| !l.trim().is_empty())
                .cloned()
                .collect();
            label_bodies.insert(label.clone(), body);
        }

        let switch_expr_label = detect_switch_expr_pattern(&cases, &label_bodies);

        let mut new_lines: Vec<String> = Vec::new();
        new_lines.push(out[i].clone());
        new_lines.push("        {".into());

        for (case_num, label) in &cases {
            new_lines.push(format!("            case {case_num}:"));
            if let Some(body) = label_bodies.get(label) {
                for bl in body {
                    let stripped = bl.trim_start();
                    if let Some(se_label) = &switch_expr_label
                        && stripped == format!("goto {se_label};") {
                            continue;
                        }
                    new_lines.push(format!("                {stripped}"));
                }
                if switch_expr_label.is_some() {
                    let has_return = body
                        .last()
                        .map(|l| l.trim().starts_with("return"))
                        .unwrap_or(false);
                    if !has_return {
                        new_lines.push("                break;".into());
                    }
                }
            }
        }

        if let Some(default_label) = &default_label
            && let Some(body) = label_bodies.get(default_label) {
                new_lines.push("            default:".into());
                for bl in body {
                    let stripped = bl.trim_start();
                    if let Some(se_label) = &switch_expr_label
                        && stripped == format!("goto {se_label};") {
                            continue;
                        }
                    new_lines.push(format!("                {stripped}"));
                }
                if switch_expr_label.is_some() {
                    let has_return = body
                        .last()
                        .map(|l| l.trim().starts_with("return"))
                        .unwrap_or(false);
                    if !has_return {
                        new_lines.push("                break;".into());
                    }
                }
            }

        new_lines.push("        }".into());

        let mut last_line = switch_close;
        let mut referenced_labels: Vec<&String> =
            cases.iter().map(|(_, l)| l).chain(default_label.as_ref()).collect();
        if let Some(se_label) = &switch_expr_label {
            referenced_labels.push(se_label);
        }
        for (idx, (label_line, label)) in all_labels.iter().enumerate() {
            if referenced_labels.contains(&label) {
                let body_end = if idx + 1 < all_labels.len() {
                    all_labels[idx + 1].0
                } else {
                    let mut end = out.len();
                    if let Some(k) = out[label_line + 1..]
                        .iter()
                        .position(|l| l.trim() == "}")
                    {
                        end = label_line + 1 + k;
                    }
                    end
                };
                if body_end > last_line {
                    last_line = body_end;
                }
            }
        }

        if let Some(se_label) = &switch_expr_label
            && let Some(body) = label_bodies.get(se_label) {
                for bl in body {
                    let stripped = bl.trim_start();
                    new_lines.push(format!("        {stripped}"));
                }
            }

        let replace_count = last_line - i;
        out.splice(i..i + replace_count, new_lines.iter().cloned());

        i += new_lines.len();
    }
}

/// Blank `Label_XXXX:` lines that no other line references (the referring
/// structures — if/else, loops, switch, EH markers — are long gone by the
/// time this runs; leftover references are raw `goto X;` lines).
pub fn remove_unreferenced_labels(out: &mut [String]) {
    for i in 0..out.len() {
        let t = out[i].trim();
        if !(t.starts_with("Label_") && t.ends_with(':')) {
            continue;
        }
        let name = t.trim_end_matches(':');
        let referenced = out
            .iter()
            .enumerate()
            .any(|(k, l)| k != i && l.contains(name));
        if !referenced {
            out[i] = String::new();
        }
    }
}

/// Remove `goto L;` immediately followed by `L:` — a jump to the very next
/// line. Both lines are only blanked when no other line still references
/// the label.
pub fn remove_redundant_jump_pairs(out: &mut [String]) {
    let mut i = 0;
    while i + 1 < out.len() {
        let t = out[i].trim();
        let Some(label) = t.strip_prefix("goto ").and_then(|s| s.strip_suffix(';')) else {
            i += 1;
            continue;
        };
        if out[i + 1].trim() != format!("{label}:") {
            i += 1;
            continue;
        }
        let referenced = out
            .iter()
            .enumerate()
            .any(|(k, l)| k != i && k != i + 1 && l.contains(label));
        if !referenced {
            out[i] = String::new();
            out[i + 1] = String::new();
            i += 2;
            continue;
        }
        i += 1;
    }
}

/// Convert leftover `goto Label_XXXX;` / `Label_XXXX:` lines into comments
/// (required for valid Java output; improves C# readability of irreducible
/// control flow).
pub fn cleanup_goto_labels(out: &mut [String]) {
    for line in out.iter_mut() {
        let t = line.trim();
        let is_goto = t.starts_with("goto ") && t.ends_with(';');
        let is_label = t.starts_with("Label_") && t.ends_with(':');
        if is_goto || is_label {
            let indent = &line[..line.len() - line.trim_start().len()];
            *line = format!("{indent}// {t}");
        }
    }
}

/// Drop lines that restructuring passes emptied out.
pub fn drop_empty_lines(out: &mut Vec<String>) {
    out.retain(|l| !l.trim().is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_paren_stripping() {
        let mut s = vec!["a + b".into(), "c".into()];
        binop(&mut s, "*");
        assert_eq!(s[0], "(a + b) * c");
        let mut s = vec!["(a + b)".into(), "c".into()];
        binop(&mut s, "+");
        assert_eq!(s[0], "a + b + c");
    }

    #[test]
    fn negate_conditions() {
        assert_eq!(negate_cond("(a >= b)"), "(a < b)");
        assert_eq!(negate_cond("!(flag)"), "(flag)");
        assert_eq!(negate_cond("(x)"), "!(x)");
    }

    #[test]
    fn if_else_restructure() {
        let mut out = vec![
            "        if (x <= 0) goto Label_0007;".into(),
            "        return 1;".into(),
            "        Label_0007:".into(),
            "        return 2;".into(),
        ];
        restructure_if_else(&mut out);
        assert_eq!(out[0], "        if (x > 0) {");
        assert_eq!(out[1], "            return 1;");
        assert_eq!(out[2], "        }");
        assert_eq!(out[3], "        return 2;");
    }

    #[test]
    fn while_loop_restructure() {
        let mut out = vec![
            "        goto Label_000E;".into(),
            "        Label_0006:".into(),
            "        i = (i + 1);".into(),
            "        Label_000E:".into(),
            "        if (i < 10) goto Label_0006;".into(),
            "        return i;".into(),
        ];
        restructure_while_loops(&mut out);
        drop_empty_lines(&mut out);
        assert_eq!(out[0], "        while (i < 10) {");
        assert_eq!(out[1], "            i = (i + 1);");
        assert_eq!(out[2], "        }");
        assert_eq!(out[3], "        return i;");
    }

    #[test]
    fn switch_restructure_with_negative_cases() {
        let mut out = vec![
            "        switch (v)".into(),
            "        {".into(),
            "            case -1: goto Label_0010;".into(),
            "            case 2: goto Label_0014;".into(),
            "        }".into(),
            "        goto Label_0018;".into(),
            "        Label_0010:".into(),
            "        return 1;".into(),
            "        Label_0014:".into(),
            "        return 2;".into(),
            "        Label_0018:".into(),
            "        return 3;".into(),
        ];
        restructure_switch(&mut out);
        let joined = out.join("\n");
        assert!(joined.contains("case -1:"));
        assert!(joined.contains("case 2:"));
        assert!(joined.contains("return 1;"));
        assert!(joined.contains("default:"));
        assert!(!joined.contains("goto Label_0010;"));
    }

    #[test]
    fn switch_restructure_adds_break() {
        // Statement switch: case bodies end with `goto` to the common
        // after-switch label — pass detects the pattern and adds `break;`.
        let mut out = vec![
            "        switch (v)".into(),
            "        {".into(),
            "            case 1: goto Label_0010;".into(),
            "            case 2: goto Label_0014;".into(),
            "        }".into(),
            "        goto Label_0018;".into(),
            "        Label_0010:".into(),
            "        v = 10;".into(),
            "        goto Label_0018;".into(),
            "        Label_0014:".into(),
            "        v = 20;".into(),
            "        goto Label_0018;".into(),
            "        Label_0018:".into(),
            "        return v;".into(),
        ];
        restructure_switch(&mut out);
        let joined = out.join("\n");
        assert!(joined.contains("break;"));
        assert!(!joined.contains("goto Label_0018;"));
    }

    #[test]
    fn leftover_gotos_become_comments() {
        let mut out = vec!["        goto Label_0003;".into(), "        Label_0003:".into()];
        cleanup_goto_labels(&mut out);
        assert_eq!(out[0], "        // goto Label_0003;");
        assert_eq!(out[1], "        // Label_0003:");
    }
}
