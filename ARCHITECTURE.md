# backtrip — Architecture

## Overview

backtrip is a pipeline with two independent front ends sharing one CLI and
one control-flow engine:

```
.NET:   .dll/.exe bytes → pe.rs → metadata/ → Reader → cil/ → decompile/csharp.rs → C#
Java:   .class bytes    → java/classfile.rs → java/decoder.rs → decompile/java.rs  → Java
                                                       ↘ java/disasm.rs        → javap-style
```

Format detection happens in `main.rs` by magic bytes: `MZ` → .NET path,
`0xCAFEBABE` → Java path. Both back ends emit a flat, goto-based statement
list that is folded into structured source by the shared passes in
`decompile/flow.rs` (if/else, while/do-while/for, switch, goto cleanup).

```
 input bytes (MZ or 0xCAFEBABE)
      |
      v  (.NET)                (Java)
   pe.rs                   java/classfile.rs  (constant pool, members, attrs)
      |                         |
      v                         v
 metadata/                 java/descriptor.rs (JVMS 4.3 types)
      |                         |
      v                         v
   Reader                   java/decoder.rs + java/opcodes.rs (JVM bytecode)
      |                         |
      +----------+--------------+
      v          v
decompile/csharp.rs  decompile/java.rs   (expression-stack machines)
      |          |    decompile/flow.rs  (shared restructuring passes)
      v          v
   output.rs  — one file per type
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

### `java` (JVM class files)

- `classfile` — `ClassFile::parse` validates the `0xCAFEBABE` magic and parses
  the constant pool (all 17 tags, `Long`/`Double` occupying two slots,
  modified UTF-8 decoding), access flags, fields, methods, and attributes.
  Structured attributes: `Code` (with exception table and LocalVariableTable),
  `ConstantValue`, `Exceptions`, `InnerClasses`, `Signature`, `SourceFile`.
  `BootstrapMethods` is kept raw for `invokedynamic` decoding.
- `descriptor` — JVMS 4.3 field/method descriptor parsing to dotted Java
  type names, plus class generic type-parameter names from `Signature`.
- `opcodes` — the complete 0x00–0xC9 JVM opcode table with operand kinds;
  branch offsets are relative to the opcode address (unlike CIL).
- `decoder` — `decode(&[u8]) -> Vec<Instruction>`; expands `wide` prefixes,
  decodes `tableswitch`/`lookupswitch` with 4-byte padding, and stores
  absolute branch targets.
- `disasm` — javap-style text output for `--il` on class files.

### `decompile`

Two back ends (`csharp.rs`, `java.rs`) plus a shared control-flow engine
(`flow.rs`):

- `flow.rs` — language-neutral passes over the flat `Vec<String>` statement
  list both back ends emit: `restructure_if_else`, `restructure_while_loops`
  (Roslyn shape), `restructure_while_loops_javac` (condition-header shape),
  `restructure_do_while_loops`, `restructure_for_loops`, `restructure_switch`,
  `cleanup_goto_labels` (comments out leftover jumps — mandatory for valid
  Java), and the precedence-aware expression helpers (`binop`, `prec`,
  `strip_outer_parens`).
- `csharp.rs` contains:
  - `decompile_assembly` — iterates `TypeDef` rows (skipping `<Module>`),
    producing one `DecompiledType` per type.
  - `decompile_type` — emits the namespace, kind (`class`/`struct`/`interface`/
    `enum`), base type + interfaces, fields, then methods.
  - `decompile_method` — renders the signature (access, `static`/`virtual`/
    `override`/`abstract`, return type, parameters, generics) and the body.
  - `decompile_body` — the expression-stack machine, plus .NET-specific
    passes (locks, using, foreach, lambdas, exception markers).
- `java.rs` mirrors that shape: `render_class`/`render_method`/`render_field`
  for declarations, and `decompile_body` — an expression-stack machine with
  Java-specific handling:
  - **join frames**: values live across branches are flushed into per-target
    temp variables (`T{offset}_{i}`), sound because the JVM verifier
    guarantees consistent stack depth per join point;
  - **`invokedynamic` string concat**: javac 9+ recipes
    (`makeConcatWithConstants`) are decoded from BootstrapMethods back into
    `+` expressions;
  - **lazy exception-var binding**: catch variables are named when the walk
    reaches the handler (javac reuses slots for locals and catch vars);
  - **ternary/boolean-return reconstruction** from javac's materialized
    branches;
  - `insert_exception_markers` — Java EH clauses have no explicit length; a
    handler region ends where the next handler starts, and javac's
    self-covering finally ranges are collapsed.

### `output`

`write_types` creates the output directory and writes one file per
`DecompiledType`, returning the count.

### `error`

`Error` (IO, invalid PE/metadata/CIL/class-file/signature, `NotImplemented`,
`NotFound`, `Usage`) + the crate-wide `Result<T>` alias used by every module.

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
