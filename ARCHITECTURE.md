# transcode — Architecture

## Overview

transcode is a pipeline: PE bytes → metadata → CIL → C# (or IL). Each stage is
a module with a clear boundary; data flows downward and is immutable once
parsed.

```
 .dll/.exe bytes
      |
      v
   pe.rs            PeImage (sections, RVA table, CLI header loc)
      |
      v
 metadata/          MetadataRoot + Tables + signatures
      |
      v
   Reader           high-level accessors (names, sigs, bodies, resolution)
      |
      v
   cil/             Instruction stream (decode) + disasm text
      |
      v
 decompile/         C# source (expression-stack machine)
      |
      v
  output.rs         one file per type
```

## Modules

### `pe`

`PeImage::parse` validates the MZ/PE signature, reads the COFF + optional
header (PE32 vs PE32+), the 16 data directories, and section headers. It
exposes `rva_to_offset` (used everywhere RVA resolution is needed) and
`cli_header` to obtain the metadata directory and entry-point token.

### `metadata`

- `streams` — `MetadataRoot` parses the `BSJB` root and stream headers, and
  provides heap accessors. `#US` user strings are decoded as UTF-16LE.
  `decode_compressed_uint` implements ECMA-335 compressed-integer decoding.
- `tables` — defines table numbers (`tbl`), coded-index kinds (`Coded`), the
  per-table column schemas (`schema`), index-size computation (`IndexSizes`),
  and the row parser (`parse_tables`). `decode_coded` turns a coded column
  value into `(Option<table>, row)`.
- `signatures` — `Type`, `MethodSig`, `ArrayShape` and the recursive
  `parse_type` / `parse_method_sig` / `parse_field_sig` decoders.
- `reader` — `Reader` borrows `PeImage`, `MetadataRoot`, and `Tables` and
  offers ergonomic accessors: type/method/field names, signatures, method
  bodies (tiny/fat), local types, generic parameters, nested-class parents,
  and C#-ish type-name rendering.

### `cil`

- `opcodes` — the complete opcode → `OpInfo` lookup (1-byte and `0xFE`-prefix
  2-byte), sourced from dotnet/runtime `opcode.def`.
- `decoder` — `decode(&[u8]) -> Vec<Instruction>`. Each `Instruction` carries
  its offset, opcode value, name, typed `Operand`, and total size.
- `disasm` — renders instructions as `IL_NNNN: op operand` text, resolving
  tokens through the `Reader`.

### `decompile`

`csharp.rs` contains:
- `decompile_assembly` — iterates `TypeDef` rows (skipping `<Module>`),
  producing one `DecompiledType` per type.
- `decompile_type` — emits the namespace, kind (`class`/`struct`/`interface`/
  `enum`), base type + interfaces, fields, then methods.
- `decompile_method` — renders the signature (access, `static`/`virtual`/
  `override`/`abstract`, return type, parameters, generics) and the body.
- `decompile_body` — the expression-stack machine. It precomputes branch
  targets, then walks instructions, maintaining `stack: Vec<String>` of C#
  expressions and `out: Vec<String>` of statements. Control flow becomes
  `Label_NNNN:` + `goto`. Unsupported opcodes emit a comment and reset.

### `output`

`write_types` creates the output directory and writes one file per
`DecompiledType`, returning the count.

### `main` / `lib`

`main.rs` is the clap CLI (`--il`, `--list`, `-o`). `lib.rs` re-exports all
modules so integration tests can exercise the pipeline directly without
shelling out.

## Key design decisions

- **Generic table schema**: rather than a typed struct per table, tables use a
  column-type list (`&[Col]`). This keeps sizing correct for every present
  table (required to find later tables' offsets) with minimal code, while
  typed accessors live in `Reader`.
- **Coded indexes decoded lazily**: raw column values are stored as `u32`;
  `decode_coded` is applied at access time. A value of `0` is "no reference".
- **Stack-machine decompiler**: a full C# decompiler needs control-flow
  graphs and expression trees. The stack-machine approach produces correct,
  readable output for straight-line code immediately and degrades gracefully
  (goto labels) for complex control flow, giving a working tool today and a
  clear path to structured reconstruction later.

## Test strategy

`tests/integration.rs` builds against `tests/fixtures/Sample.cs` (a small C#
library compiled with `dotnet`) and asserts that decompiled output contains
expected C# fragments (class/struct kinds, method signatures, arithmetic
bodies, string concatenation, constructors, field access, `Math.Sqrt` calls).
Unit tests cover the CIL decoder and compressed-integer decoding.
