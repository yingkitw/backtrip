use std::path::PathBuf;

use roundtrip::decompile::{decompile_assembly, decompile_type_by_name};
use roundtrip::metadata::{load, Reader};
use roundtrip::pe::PeImage;

fn sample_dll() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/Release/net8.0/Sample.dll")
}

fn load_reader() -> (PeImage, roundtrip::metadata::streams::MetadataRoot, roundtrip::metadata::tables::Tables) {
    let data = std::fs::read(sample_dll()).expect("read sample dll");
    let pe = PeImage::parse(data).expect("parse PE");
    let (root, tables) = load(&pe).expect("load metadata");
    (pe, root, tables)
}

#[test]
fn parses_pe_and_metadata() {
    let (pe, _root, tables) = load_reader();
    let cli = pe.cli_header().unwrap();
    assert!(cli.metadata_size > 0);
    // Assembly table should have exactly 1 row.
    assert_eq!(tables.row_count(roundtrip::metadata::tbl::ASSEMBLY), 1);
}

#[test]
fn lists_expected_types() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let type_defs = reader.tables.get(roundtrip::metadata::tbl::TYPEDEF);
    let names: Vec<String> = type_defs
        .iter()
        .map(|r| reader.type_def_name(r))
        .filter(|n| n != "<Module>")
        .collect();
    assert!(names.contains(&"Calculator".to_string()));
    assert!(names.contains(&"Point".to_string()));
}

#[test]
fn decompiles_calculator_add_body() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    assert!(calc.source.contains("public class Calculator"));
    assert!(calc.source.contains("public int Add(int a, int b)"));
    assert!(calc.source.contains("return (a + b);"));
    assert!(calc.source.contains("public static int Square(int x)"));
    assert!(calc.source.contains("return (x * x);"));
    // const field: Literal flag + Constant table value.
    assert!(calc.source.contains("public const int MaxValue = 100;"));
    // Property: auto-property rendered as { get; set; }, not get_/set_ methods.
    assert!(calc.source.contains("public string Label { get; set; }"));
    assert!(!calc.source.contains("get_Label"), "getter method should be skipped");
    assert!(!calc.source.contains("set_Label"), "setter method should be skipped");
    // Event: rendered as `event Notify OnCalculating;`, not add_/remove_ methods.
    assert!(calc.source.contains("public event Notify OnCalculating;"));
    assert!(!calc.source.contains("add_OnCalculating"), "add method should be skipped");
    assert!(!calc.source.contains("remove_OnCalculating"), "remove method should be skipped");
    // Custom attribute: [Obsolete] on the type, with constructor argument.
    assert!(calc.source.contains("[Obsolete(\"Use NewCalculator instead.\")]"),
        "attribute with args missing;\n{}", calc.source);
}

#[test]
fn decompiles_explicit_interface_impl() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let counter = types
        .iter()
        .find(|t| t.file_name.contains("Counter"))
        .expect("Counter file");
    // Interface in base list should use simple name (same namespace).
    assert!(counter.source.contains("public class Counter : IResettable"),
        "interface base list wrong;\n{}", counter.source);
    // Explicit interface impl: `void IResettable.Reset()` — no access modifier.
    assert!(counter.source.contains("void IResettable.Reset()"),
        "explicit impl missing;\n{}", counter.source);
    // Must not render as `private virtual void Shapes.IResettable.Reset()`.
    assert!(!counter.source.contains("private virtual"),
        "explicit impl should not have access/virtual modifiers;\n{}", counter.source);
    // P/Invoke: [DllImport] + extern, no body.
    assert!(counter.source.contains("[DllImport(\"libc\")]"),
        "DllImport attribute missing;\n{}", counter.source);
    assert!(counter.source.contains("public static extern int getpid();"),
        "P/Invoke method missing;\n{}", counter.source);
    // Field-level attribute: [Obsolete] on Count.
    assert!(counter.source.contains("[Obsolete]\n    public int Count"),
        "field attribute missing;\n{}", counter.source);
    // Method-level attribute: [Obsolete("Use IncrementBy instead.")] on Increment.
    assert!(counter.source.contains("[Obsolete(\"Use IncrementBy instead.\")]"),
        "method attribute with args missing;\n{}", counter.source);
}

#[test]
fn decompiles_default_parameter() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // Default parameter: `int c = 0` in the 3-arg Add overload.
    assert!(calc.source.contains("int c = 0"),
        "default parameter missing;\n{}", calc.source);
}

#[test]
fn decompiles_switch_statement() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // Switch: should render as `switch (day)` with `case N: goto` entries.
    assert!(calc.source.contains("switch (day)"),
        "switch statement missing;\n{}", calc.source);
    assert!(calc.source.contains("case 0: goto"),
        "switch case 0 missing;\n{}", calc.source);
    assert!(calc.source.contains("case 6: goto"),
        "switch case 6 missing;\n{}", calc.source);
    // Must not render as `if (day == 0) goto` chain.
    assert!(!calc.source.contains("if (day == 0) goto"),
        "should use switch, not if-chain;\n{}", calc.source);
}

#[test]
fn decompiles_try_catch() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // try/catch: should render `try {` and `catch (FormatException) {`.
    assert!(calc.source.contains("try"), "try block missing;\n{}", calc.source);
    assert!(calc.source.contains("catch (FormatException)"),
        "catch clause missing;\n{}", calc.source);
}

#[test]
fn decompiles_if_else() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // if/else: should render `if (x <= 0) {` with a block, not `if (x >= 0) goto`.
    assert!(calc.source.contains("if (x <= 0) {"),
        "if block missing;\n{}", calc.source);
    assert!(calc.source.contains("return (-x);"),
        "if body missing;\n{}", calc.source);
    // Must not render as `if (x >= 0) goto Label_`.
    assert!(!calc.source.contains("if (x >= 0) goto"),
        "should use if block, not goto;\n{}", calc.source);
}

#[test]
fn decompiles_string_concat() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .unwrap();
    assert!(calc.source.contains("String.Concat(\"Hello, \", name, \"!\")"));
}

#[test]
fn decompiles_struct_point() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let point = types
        .iter()
        .find(|t| t.file_name.contains("Point"))
        .unwrap();
    assert!(point.source.contains("public struct Point"));
    assert!(point.source.contains("public int X;"));
    assert!(point.source.contains("public int Y;"));
    assert!(point.source.contains("Math.Sqrt"));
}

#[test]
fn decompiles_enum_with_values() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let color = types
        .iter()
        .find(|t| t.file_name.contains("Color"))
        .expect("Color enum file");
    assert!(color.source.contains("public enum Color : int"));
    assert!(color.source.contains("Red = 0"));
    assert!(color.source.contains("Green = 1"));
    assert!(color.source.contains("Blue = 2"));
    // The synthetic value__ field must not appear as a member.
    assert!(!color.source.contains("value__"));
}

#[test]
fn decompiles_nested_type_inside_parent() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    // Settings should NOT get its own file — it's nested inside Calculator.
    assert!(types.iter().all(|t| !t.file_name.contains("Settings")),
        "Settings should not have its own file");
    // Calculator's source should contain the nested Settings class.
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    assert!(calc.source.contains("class Settings"), "nested Settings class missing;\n{}", calc.source);
    assert!(calc.source.contains("public Settings(string label)"), "nested Settings ctor missing");
    assert!(calc.source.contains("this.Enabled = 1"), "nested Settings ctor body missing");
}

#[test]
fn decompiles_delegate() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let notify = types
        .iter()
        .find(|t| t.file_name.contains("Notify"))
        .expect("Notify delegate file");
    assert!(notify.source.contains("public delegate void Notify(string message);"),
        "delegate not rendered correctly;\n{}", notify.source);
    // Must not render as a class with MulticastDelegate base.
    assert!(!notify.source.contains("MulticastDelegate"));
    assert!(!notify.source.contains("Invoke"));
}

#[test]
fn decompiles_constructor() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .unwrap();
    assert!(calc.source.contains("public Calculator(int value)"));
    assert!(calc.source.contains("this.Value = value;"));
    // Calculator implicitly extends object; the implicit base() call to
    // System.Object's constructor should be suppressed.
    assert!(!calc.source.contains("base();"));
}

#[test]
fn cil_decoder_roundtrip_sizes() {
    // A tiny method body: ldarg.1, ldarg.2, add, ret.
    let code = [0x03_u8, 0x04, 0x58, 0x2A];
    let instrs = roundtrip::cil::decode(&code).unwrap();
    assert_eq!(instrs.len(), 4);
    assert_eq!(instrs[0].name, "ldarg.1");
    assert_eq!(instrs[2].name, "add");
    assert_eq!(instrs[3].name, "ret");
    // Total size should equal the input length.
    let total: usize = instrs.iter().map(|i| i.size).sum();
    assert_eq!(total, code.len());
}

#[test]
fn cil_decoder_ldc_and_branch() {
    // ldc.i4.s 42, brtrue.s +0, ret
    let code = [0x1F_u8, 42, 0x2D, 0x00, 0x2A];
    let instrs = roundtrip::cil::decode(&code).unwrap();
    assert_eq!(instrs[0].name, "ldc.i4.s");
    assert_eq!(instrs[1].name, "brtrue.s");
    assert_eq!(instrs[2].name, "ret");
    let total: usize = instrs.iter().map(|i| i.size).sum();
    assert_eq!(total, code.len());
}

#[test]
fn decompile_single_type_by_simple_name() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let t = decompile_type_by_name(&reader, "Calculator")
        .unwrap()
        .expect("Calculator found by simple name");
    assert!(t.file_name.contains("Calculator"));
    assert!(t.source.contains("public class Calculator"));
    // Only one type is returned — Point must not be present.
    assert!(!t.source.contains("struct Point"));
}

#[test]
fn decompile_single_type_by_full_name() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let t = decompile_type_by_name(&reader, "Shapes.Point")
        .unwrap()
        .expect("Point found by fully-qualified name");
    assert!(t.source.contains("public struct Point"));
}

#[test]
fn decompile_single_type_not_found() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    assert!(decompile_type_by_name(&reader, "DoesNotExist").unwrap().is_none());
}

#[test]
fn compressed_uint_decoding() {
    use roundtrip::metadata::streams::decode_compressed_uint;
    assert_eq!(decode_compressed_uint(&[0x05]).unwrap(), (5, 1));
    assert_eq!(decode_compressed_uint(&[0x80, 0x05]).unwrap(), (5, 2));
    assert_eq!(decode_compressed_uint(&[0xC0, 0x00, 0x01, 0x00]).unwrap(), (256, 4));
}

// ---- CLI smoke tests (shell out to the built binary) ----

fn roundtrip_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_roundtrip"))
}

#[test]
fn cli_stdout_prints_single_type() {
    let out = std::process::Command::new(roundtrip_bin())
        .arg(sample_dll())
        .arg("--type")
        .arg("Calculator")
        .arg("--stdout")
        .output()
        .expect("run roundtrip");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("public class Calculator"));
    // Only one type printed — Point must not appear.
    assert!(!stdout.contains("struct Point"));
}

#[test]
fn cli_stdout_requires_type() {
    let out = std::process::Command::new(roundtrip_bin())
        .arg(sample_dll())
        .arg("--stdout")
        .output()
        .expect("run roundtrip");
    assert!(!out.status.success(), "--stdout without --type should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--stdout requires --type"));
}

#[test]
fn il_disasm_resolves_ldstr_and_locals() {
    let out = std::process::Command::new(roundtrip_bin())
        .arg(sample_dll())
        .arg("--il")
        .arg("--type")
        .arg("Calculator")
        .arg("--stdout")
        .output()
        .expect("run roundtrip");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let il = String::from_utf8_lossy(&out.stdout);
    // ldstr should resolve to the literal string, not a raw token.
    assert!(il.contains("\"Hello, \""), "ldstr not resolved to literal; got:\n{il}");
    // Locals should be named V_0 etc. and declared in a .locals header.
    assert!(il.contains(".locals init"), "missing .locals header; got:\n{il}");
    assert!(il.contains("V_0"), "locals not named V_0; got:\n{il}");
}
