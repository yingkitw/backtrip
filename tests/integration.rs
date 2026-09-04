use std::path::PathBuf;

use transcode::decompile::decompile_assembly;
use transcode::metadata::{load, Reader};
use transcode::pe::PeImage;

fn sample_dll() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/Release/net8.0/Sample.dll")
}

fn load_reader() -> (PeImage, transcode::metadata::streams::MetadataRoot, transcode::metadata::tables::Tables) {
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
    assert_eq!(tables.row_count(transcode::metadata::tbl::ASSEMBLY), 1);
}

#[test]
fn lists_expected_types() {
    let (_pe, root, tables) = load_reader();
    let reader = Reader::new(&_pe, &root, &tables).unwrap();
    let type_defs = reader.tables.get(transcode::metadata::tbl::TYPEDEF);
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
}

#[test]
fn cil_decoder_roundtrip_sizes() {
    // A tiny method body: ldarg.1, ldarg.2, add, ret.
    let code = [0x03_u8, 0x04, 0x58, 0x2A];
    let instrs = transcode::cil::decode(&code).unwrap();
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
    let instrs = transcode::cil::decode(&code).unwrap();
    assert_eq!(instrs[0].name, "ldc.i4.s");
    assert_eq!(instrs[1].name, "brtrue.s");
    assert_eq!(instrs[2].name, "ret");
    let total: usize = instrs.iter().map(|i| i.size).sum();
    assert_eq!(total, code.len());
}

#[test]
fn compressed_uint_decoding() {
    use transcode::metadata::streams::decode_compressed_uint;
    assert_eq!(decode_compressed_uint(&[0x05]).unwrap(), (5, 1));
    assert_eq!(decode_compressed_uint(&[0x80, 0x05]).unwrap(), (5, 2));
    assert_eq!(decode_compressed_uint(&[0xC0, 0x00, 0x01, 0x00]).unwrap(), (256, 4));
}
