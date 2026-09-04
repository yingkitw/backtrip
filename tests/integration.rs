use std::path::PathBuf;

use backtrip::decompile::{decompile_assembly, decompile_type_by_name};
use backtrip::metadata::{load, Reader};
use backtrip::pe::PeImage;

fn sample_dll() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/Release/net8.0/Sample.dll")
}

fn load_reader() -> (PeImage, backtrip::metadata::streams::MetadataRoot, backtrip::metadata::tables::Tables) {
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
    assert_eq!(tables.row_count(backtrip::metadata::tbl::ASSEMBLY), 1);
}

#[test]
fn lists_expected_types() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let type_defs = reader.tables.get(backtrip::metadata::tbl::TYPEDEF);
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
    assert!(calc.source.contains("return a + b;"));
    assert!(calc.source.contains("public static int Square(int x)"));
    assert!(calc.source.contains("return x * x;"));
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
    // Switch: should render as `switch (day)` with inlined case bodies.
    assert!(calc.source.contains("switch (day)"),
        "switch statement missing;\n{}", calc.source);
    assert!(calc.source.contains("case 0:"),
        "switch case 0 missing;\n{}", calc.source);
    assert!(calc.source.contains("case 6:"),
        "switch case 6 missing;\n{}", calc.source);
    // Should have inlined case bodies (return statements inside cases).
    assert!(calc.source.contains("return \"Sunday\";"),
        "case 0 body should be inlined;\n{}", calc.source);
    assert!(calc.source.contains("return \"Saturday\";"),
        "case 6 body should be inlined;\n{}", calc.source);
    // Should have a default case.
    assert!(calc.source.contains("default:"),
        "default case missing;\n{}", calc.source);
    assert!(calc.source.contains("return \"Unknown\";"),
        "default body should be inlined;\n{}", calc.source);
    // Must not render as `if (day == 0) goto` chain.
    assert!(!calc.source.contains("if (day == 0) goto"),
        "should use switch, not if-chain;\n{}", calc.source);
    // Must not have goto labels for cases.
    assert!(!calc.source.contains("case 0: goto"),
        "should not have goto in cases;\n{}", calc.source);
}

#[test]
fn decompiles_switch_expression() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // Switch expression compiles to a switch with V_0 = ...; goto Label; pattern.
    // Should be reconstructed as a switch with break; and return V_0;.
    assert!(calc.source.contains("ClassifyDay"),
        "ClassifyDay method missing;\n{}", calc.source);
    assert!(calc.source.contains("V_0 = \"Sunday\";"),
        "switch expression case body should be inlined;\n{}", calc.source);
    assert!(calc.source.contains("V_0 = \"Unknown\";"),
        "switch expression default body should be inlined;\n{}", calc.source);
    // Should have break; after each case (switch expression pattern).
    assert!(calc.source.contains("break;"),
        "switch expression should have break;\n{}", calc.source);
    // Should not have goto labels for cases.
    assert!(!calc.source.contains("goto Label_0062"),
        "switch expression should not have goto to end label;\n{}", calc.source);
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
    // if/else: should render `if (x < 0) {` with a block, not `if (x >= 0) goto`.
    assert!(calc.source.contains("if (x < 0) {"),
        "if block missing;\n{}", calc.source);
    assert!(calc.source.contains("return -x;"),
        "if body missing;\n{}", calc.source);
    // Must not render as `if (x >= 0) goto Label_`.
    assert!(!calc.source.contains("if (x >= 0) goto"),
        "should use if block, not goto;\n{}", calc.source);
}

#[test]
fn decompiles_while_loop() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // while/for loop: SumUpTo has init+increment so it's upgraded to a for loop.
    assert!(calc.source.contains("for (V_1 = 1; V_1 <= n; V_1 = V_1 + 1) {"),
        "for loop (from while) missing;\n{}", calc.source);
    // Must not render the loop as `goto Label_` + `if ... goto` back-edge.
    assert!(!calc.source.contains("goto Label_000E;\n        Label_0006:"),
        "should use for, not goto for loop;\n{}", calc.source);
}

#[test]
fn decompiles_do_while_loop() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // do-while: should render `do {` ... `} while (start > 0);`
    assert!(calc.source.contains("do {"),
        "do block missing;\n{}", calc.source);
    assert!(calc.source.contains("} while (start > 0);"),
        "do-while condition missing;\n{}", calc.source);
    // Must not render as `Label_0002:` + `if ... goto Label_0002;`
    assert!(!calc.source.contains("if (start > 0) goto Label_0002;"),
        "should use do-while, not goto;\n{}", calc.source);
}

#[test]
fn decompiles_for_loop() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // for loop: should render `for (V_1 = 1; V_1 <= n; V_1 = V_1 + 1) {`
    assert!(calc.source.contains("for (V_1 = 1; V_1 <= n; V_1 = V_1 + 1) {"),
        "for loop missing;\n{}", calc.source);
    // Must not render as `while (V_1 <= n) {` (should be upgraded to for).
    assert!(!calc.source.contains("while (V_1 <= n)"),
        "should use for, not while;\n{}", calc.source);
}

#[test]
fn decompiles_ref_out_parameters() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // ref parameters: `ref int a, ref int b`
    assert!(calc.source.contains("ref int a, ref int b"),
        "ref parameters missing;\n{}", calc.source);
    // out parameters: `out int result`
    assert!(calc.source.contains("out int result"),
        "out parameter missing;\n{}", calc.source);
    // Must not render out as ref.
    assert!(!calc.source.contains("ref int result"),
        "out parameter should not render as ref;\n{}", calc.source);
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
    // String.Concat should be reconstructed as + operators.
    assert!(calc.source.contains("\"Hello, \" + name + \"!\""),
        "string concat not reconstructed as +;\n{}", calc.source);
    assert!(!calc.source.contains("String.Concat("),
        "should use +, not String.Concat;\n{}", calc.source);
}

#[test]
fn decompiles_auto_properties() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // Auto-properties should render with get/set accessors.
    assert!(calc.source.contains("public string ReadOnly { get; }"),
        "read-only auto-property missing;\n{}", calc.source);
    assert!(calc.source.contains("public int Count { get; set; }"),
        "read-write auto-property missing;\n{}", calc.source);
    // Backing field names should NOT appear in the output.
    assert!(!calc.source.contains("k__BackingField"),
        "backing field name should be cleaned;\n{}", calc.source);
    // Constructor should reference the property name, not the backing field.
    assert!(calc.source.contains("this.Count = 42;"),
        "constructor should use property name;\n{}", calc.source);
    assert!(calc.source.contains("this.ReadOnly = \"default\";"),
        "constructor should use property name for read-only;\n{}", calc.source);
}

#[test]
fn decompiles_lock_block() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // lock: should render `lock (this._sync) {`
    assert!(calc.source.contains("lock (this._sync) {"),
        "lock block missing;\n{}", calc.source);
    // Must not render as Monitor.Enter/Monitor.Exit.
    assert!(!calc.source.contains("Monitor.Enter("),
        "should use lock, not Monitor.Enter;\n{}", calc.source);
    assert!(!calc.source.contains("Monitor.Exit("),
        "should use lock, not Monitor.Exit;\n{}", calc.source);
}

#[test]
fn decompiles_using_block() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // using: should render `using (V_0 = new IO.StreamReader(path)) {`
    assert!(calc.source.contains("using (V_0 = new IO.StreamReader(path)) {"),
        "using block missing;\n{}", calc.source);
    // Must not render as Dispose() in a finally block.
    assert!(!calc.source.contains(".Dispose()"),
        "should use using, not Dispose;\n{}", calc.source);
}

#[test]
fn decompiles_foreach() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // foreach: should render `foreach (var V_2 in numbers) {`
    assert!(calc.source.contains("foreach (var V_2 in numbers) {"),
        "foreach block missing;\n{}", calc.source);
    // Must not render as GetEnumerator/MoveNext/get_Current.
    assert!(!calc.source.contains("GetEnumerator()"),
        "should use foreach, not GetEnumerator;\n{}", calc.source);
    assert!(!calc.source.contains("MoveNext()"),
        "should use foreach, not MoveNext;\n{}", calc.source);
}

#[test]
fn decompiles_collection_initializer() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // collection initializer: should render `new ...() { 1, 2, 3 }`
    assert!(calc.source.contains("{ 1, 2, 3 }"),
        "collection initializer missing;\n{}", calc.source);
    // Must not render as separate Add() calls.
    assert!(!calc.source.contains(".Add(1);"),
        "should use initializer, not Add;\n{}", calc.source);
}

#[test]
fn decompiles_ref_param_body() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // ref parameter body: Swap should use dereference syntax, not unsupported comments.
    assert!(calc.source.contains("V_0 = *a;"),
        "ldind not rendered;\n{}", calc.source);
    assert!(calc.source.contains("*a = *b;"),
        "stind not rendered;\n{}", calc.source);
    // Must not have unsupported comments for ldind/stind.
    assert!(!calc.source.contains("unsupported: ldind"),
        "ldind should be supported;\n{}", calc.source);
    assert!(!calc.source.contains("unsupported: stind"),
        "stind should be supported;\n{}", calc.source);
}

#[test]
fn decompiles_type_name_cleanup() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // FCL types should map to C# keywords.
    assert!(calc.source.contains("new object()"),
        "Object should map to object keyword;\n{}", calc.source);
    // No backtick in generic type names.
    assert!(!calc.source.contains("`1<"),
        "backtick arity should be stripped;\n{}", calc.source);
    // No System. prefix (should be stripped).
    assert!(!calc.source.contains("System."),
        "System. prefix should be stripped;\n{}", calc.source);
    // Generic types should render cleanly.
    assert!(calc.source.contains("List<int>"),
        "generic type should render cleanly;\n{}", calc.source);
}

#[test]
fn decompiles_is_as_patterns() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // isinst should render as `as` pattern without redundant parens.
    assert!(calc.source.contains("obj as string"),
        "as pattern missing;\n{}", calc.source);
    // No double negation from brfalse + if/else restructuring.
    assert!(!calc.source.contains("!(!"),
        "double negation should be eliminated;\n{}", calc.source);
    // ldloca should not render as `ref` for method calls.
    assert!(calc.source.contains("V_1.ToString()"),
        "ldloca method call should not have ref;\n{}", calc.source);
    assert!(!calc.source.contains("ref V_1.ToString()"),
        "ldloca should not render as ref for method calls;\n{}", calc.source);
}

#[test]
fn decompiles_display_class_cleanup() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // Display class names should be cleaned up.
    assert!(calc.source.contains("DisplayClass"),
        "display class name should be cleaned up;\n{}", calc.source);
    assert!(!calc.source.contains("<>c__DisplayClass"),
        "raw display class name should not appear;\n{}", calc.source);
    // Lambda method names should be cleaned up.
    assert!(calc.source.contains("lambda_0"),
        "lambda method name should be cleaned up;\n{}", calc.source);
    assert!(!calc.source.contains("b__0"),
        "raw lambda method name should not appear;\n{}", calc.source);
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
fn decompiles_abstract_class_hierarchy() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();

    let shape = types
        .iter()
        .find(|t| t.file_name.ends_with("_Shape.cs"))
        .expect("Shape file");
    assert!(shape.source.contains("public abstract class Shape"),
        "abstract class missing;\n{}", shape.source);
    assert!(shape.source.contains("public abstract double Area();"),
        "abstract method missing;\n{}", shape.source);
    assert!(shape.source.contains("public virtual string Describe()"),
        "virtual method missing;\n{}", shape.source);

    let circle = types
        .iter()
        .find(|t| t.file_name.ends_with("_Circle.cs"))
        .expect("Circle file");
    assert!(circle.source.contains("public class Circle : Shapes.Shape"),
        "base class missing;\n{}", circle.source);
    assert!(circle.source.contains("public Circle(double r)"),
        "Circle ctor missing;\n{}", circle.source);
    assert!(circle.source.contains("public override double Area()"),
        "override method missing;\n{}", circle.source);
    assert!(circle.source.contains("public override string Describe()"),
        "override Describe missing;\n{}", circle.source);
}

#[test]
fn decompiles_interface_type() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let iface = types
        .iter()
        .find(|t| t.file_name.contains("IResettable"))
        .expect("IResettable file");
    assert!(iface.source.contains("public interface IResettable"),
        "interface declaration missing;\n{}", iface.source);
    assert!(iface.source.contains("void Reset();"),
        "interface method missing;\n{}", iface.source);
}

#[test]
fn decompiles_generic_class_and_method() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let box_ = types
        .iter()
        .find(|t| t.file_name.contains("Box"))
        .expect("Box file");
    // Class-level generic params render from the GenericParam table names.
    assert!(box_.source.contains("public class Box<T>"),
        "generic class missing;\n{}", box_.source);
    // Class generic params appear as T0 in member signatures (index-based).
    assert!(box_.source.contains("public T0 Item;"),
        "generic field missing;\n{}", box_.source);
    assert!(box_.source.contains("public Box(T0 item)"),
        "generic ctor missing;\n{}", box_.source);
    assert!(box_.source.contains("public T0 Get()"),
        "generic method return missing;\n{}", box_.source);
    // Method-level generic: <U> decl + !!0 in the signature.
    assert!(box_.source.contains("public !!0 Map<U>(Func<T0, !!0> f)"),
        "generic method signature missing;\n{}", box_.source);
    assert!(box_.source.contains("f.Invoke(this.Item)"),
        "delegate invocation missing;\n{}", box_.source);
    // No raw arity backticks anywhere in the rendered source.
    assert!(!box_.source.contains('`'),
        "generic arity backtick should be stripped;\n{}", box_.source);
}

#[test]
fn decompiles_static_constructor_and_field() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let logger = types
        .iter()
        .find(|t| t.file_name.contains("Logger"))
        .expect("Logger file");
    assert!(logger.source.contains("public static int InstanceCount;"),
        "static field missing;\n{}", logger.source);
    assert!(logger.source.contains("static Logger()"),
        "static ctor missing;\n{}", logger.source);
    assert!(logger.source.contains("InstanceCount = 1;"),
        "static ctor body missing;\n{}", logger.source);
}

#[test]
fn decompiles_flags_enum() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let perms = types
        .iter()
        .find(|t| t.file_name.contains("Permissions"))
        .expect("Permissions file");
    assert!(perms.source.contains("[Flags]"),
        "Flags attribute missing;\n{}", perms.source);
    assert!(perms.source.contains("public enum Permissions : int"),
        "enum declaration missing;\n{}", perms.source);
    assert!(perms.source.contains("None = 0"),
        "enum member None missing;\n{}", perms.source);
    assert!(perms.source.contains("Read = 1"),
        "enum member Read missing;\n{}", perms.source);
    assert!(perms.source.contains("Write = 2"),
        "enum member Write missing;\n{}", perms.source);
}

#[test]
fn decompiles_array_operations() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // newarr → `new int[n]`
    assert!(calc.source.contains("public int[] MakeArray(int n)"),
        "array-returning method missing;\n{}", calc.source);
    assert!(calc.source.contains("return new int[n];"),
        "newarr missing;\n{}", calc.source);
    // ldelem → `xs[0]`
    assert!(calc.source.contains("public int FirstElement(int[] xs)"),
        "array param method missing;\n{}", calc.source);
    assert!(calc.source.contains("return xs[0];"),
        "ldelem missing;\n{}", calc.source);
    // ldlen → `xs.Length`
    assert!(calc.source.contains("public int ArrayLength(int[] xs)"),
        "ldlen method missing;\n{}", calc.source);
    assert!(calc.source.contains("xs.Length"),
        "ldlen missing;\n{}", calc.source);
}

#[test]
fn decompiles_boxing_and_casting() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let types = decompile_assembly(&reader).unwrap();
    let calc = types
        .iter()
        .find(|t| t.file_name.contains("Calculator"))
        .expect("Calculator file");
    // box → `(object)(x)`
    assert!(calc.source.contains("public object BoxInt(int x)"),
        "boxing method missing;\n{}", calc.source);
    assert!(calc.source.contains("return (object)(x);"),
        "box missing;\n{}", calc.source);
    // unbox.any → `(int)(o)`
    assert!(calc.source.contains("public int UnboxInt(object o)"),
        "unboxing method missing;\n{}", calc.source);
    assert!(calc.source.contains("return (int)(o);"),
        "unbox.any missing;\n{}", calc.source);
    // castclass → `(string)(o)`
    assert!(calc.source.contains("public string CastString(object o)"),
        "cast method missing;\n{}", calc.source);
    assert!(calc.source.contains("return (string)(o);"),
        "castclass missing;\n{}", calc.source);
}

#[test]
fn cil_decoder_round_trip_sizes() {
    // A tiny method body: ldarg.1, ldarg.2, add, ret.
    let code = [0x03_u8, 0x04, 0x58, 0x2A];
    let instrs = backtrip::cil::decode(&code).unwrap();
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
    let instrs = backtrip::cil::decode(&code).unwrap();
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
    use backtrip::metadata::streams::decode_compressed_uint;
    assert_eq!(decode_compressed_uint(&[0x05]).unwrap(), (5, 1));
    assert_eq!(decode_compressed_uint(&[0x80, 0x05]).unwrap(), (5, 2));
    assert_eq!(decode_compressed_uint(&[0xC0, 0x00, 0x01, 0x00]).unwrap(), (256, 4));
}

// ---- CLI smoke tests (shell out to the built binary) ----

fn backtrip_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_backtrip"))
}

#[test]
fn cli_stdout_prints_single_type() {
    let out = std::process::Command::new(backtrip_bin())
        .arg(sample_dll())
        .arg("--type")
        .arg("Calculator")
        .arg("--stdout")
        .output()
        .expect("run backtrip");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("public class Calculator"));
    // Only one type printed — Point must not appear.
    assert!(!stdout.contains("struct Point"));
}

#[test]
fn cli_stdout_requires_type() {
    let out = std::process::Command::new(backtrip_bin())
        .arg(sample_dll())
        .arg("--stdout")
        .output()
        .expect("run backtrip");
    assert!(!out.status.success(), "--stdout without --type should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--stdout requires --type"));
}

#[test]
fn il_disasm_resolves_ldstr_and_locals() {
    let out = std::process::Command::new(backtrip_bin())
        .arg(sample_dll())
        .arg("--il")
        .arg("--type")
        .arg("Calculator")
        .arg("--stdout")
        .output()
        .expect("run backtrip");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let il = String::from_utf8_lossy(&out.stdout);
    // ldstr should resolve to the literal string, not a raw token.
    assert!(il.contains("\"Hello, \""), "ldstr not resolved to literal; got:\n{il}");
    // Locals should be named V_0 etc. and declared in a .locals header.
    assert!(il.contains(".locals init"), "missing .locals header; got:\n{il}");
    assert!(il.contains("V_0"), "locals not named V_0; got:\n{il}");
}

#[test]
fn cli_recursive_decompiles_directory() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/Release/net8.0");
    let out_dir = std::env::temp_dir().join("backtrip_recursive_test");
    // Clean up any previous run.
    let _ = std::fs::remove_dir_all(&out_dir);

    let out = std::process::Command::new(backtrip_bin())
        .arg(&dir)
        .arg("--recursive")
        .arg("-o")
        .arg(&out_dir)
        .output()
        .expect("run backtrip");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Decompiled 1 assemblies"),
        "should decompile 1 assembly; got:\n{stdout}");
    // The output directory should contain a subdirectory for the assembly.
    let sample_dir = out_dir.join("Sample");
    assert!(sample_dir.exists(), "Sample output dir should exist");
    // Should contain decompiled .cs files.
    let entries: Vec<_> = std::fs::read_dir(&sample_dir).unwrap().collect();
    assert!(!entries.is_empty(), "should have decompiled files");
    // Verify at least one .cs file exists.
    let has_cs = entries.iter().any(|e| {
        e.as_ref().unwrap().path().extension().map_or(false, |ext| ext == "cs")
    });
    assert!(has_cs, "should have .cs files in output");
}

#[test]
fn cli_json_outputs_valid_model() {
    let out = std::process::Command::new(backtrip_bin())
        .arg(sample_dll())
        .arg("--json")
        .output()
        .expect("run backtrip");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let json = String::from_utf8_lossy(&out.stdout);
    // Should be valid JSON (parse with python).
    let parsed = std::process::Command::new("python3")
        .arg("-m").arg("json.tool")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn();
    if let Ok(mut child) = parsed {
        use std::io::Write;
        child.stdin.as_mut().unwrap().write_all(json.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "JSON should be valid");
    }
    // Should contain expected type and method names.
    assert!(json.contains("\"Calculator\""), "should contain Calculator type");
    assert!(json.contains("\"namespace\": \"Shapes\""), "should contain Shapes namespace");
    assert!(json.contains("\"Add\""), "should contain Add method");
    assert!(json.contains("\"returnType\": \"int\""), "should contain int return type");
    assert!(json.contains("\"fields\""), "should have fields array");
    assert!(json.contains("\"methods\""), "should have methods array");
}

#[test]
fn cli_detect_obfuscation_clean_assembly() {
    let out = std::process::Command::new(backtrip_bin())
        .arg(sample_dll())
        .arg("--detect-obfuscation")
        .output()
        .expect("run backtrip");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8_lossy(&out.stdout);
    // Clean assembly should report no obfuscation indicators.
    assert!(report.contains("Obfuscation detection report"),
        "should have report header;\n{}", report);
    assert!(report.contains("No obfuscation indicators found"),
        "clean assembly should have no warnings;\n{}", report);
    // Should report the number of types and methods scanned.
    assert!(report.contains("types") && report.contains("methods scanned"),
        "should report scan summary;\n{}", report);
}

#[test]
fn cli_verify_clean_assembly() {
    let out = std::process::Command::new(backtrip_bin())
        .arg(sample_dll())
        .arg("--verify")
        .output()
        .expect("run backtrip");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8_lossy(&out.stdout);
    // Should have a verification report header.
    assert!(report.contains("Verification report"),
        "should have report header;\n{}", report);
    // Clean assembly should have 0 missing items.
    assert!(report.contains("0 missing"),
        "clean assembly should have 0 missing;\n{}", report);
    // Should report the number of types, methods, and fields verified.
    assert!(report.contains("Verified") && report.contains("types"),
        "should report verification summary;\n{}", report);
}
