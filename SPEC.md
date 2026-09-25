# backtrip — Specification

## Purpose

Decompile .NET assemblies (PE/CLI, ECMA-335) into C# source or CIL
disassembly, and JVM class files (JVMS) into Java source or javap-style
disassembly.

## Inputs

- **.NET**: a single PE file (`.dll` or `.exe`) containing a .NET CLI runtime
  header (PE data directory entry 14). Non-managed PE files are rejected with
  `invalid PE: no CLI runtime header; not a managed .NET assembly`.
- **Java**: a class file starting with the `0xCAFEBABE` magic, or a `.jar`
  archive (`PK\x03\x04`) containing class entries (stored or deflated; Zip64
  not supported). Files are detected by magic bytes, not extension.

## CLI

```
backtrip <INPUT> [-o <DIR>] [--il] [--list] [--type <NAME>] [--stdout]
         [--recursive] [--json] [--detect-obfuscation] [--verify]
```

`INPUT` is an assembly, a class file, or (with `--recursive`) a directory.

- Default action: decompile — C# (one `.cs` file per type) for .NET inputs,
  Java (one `.java` file) for class files — written to the output directory
  (default `decompiled`).
- `--il`: for .NET, write `.il` disassembly files; for Java, write
  javap-style `.jbc` disassembly files.
- `--list`: print fully-qualified type/class names to stdout and exit.
- `--type <NAME>`: decompile only the type/class whose simple name
  (`Calculator`) or fully-qualified name (`Shapes.Calculator` / `demo.Sample`)
  matches. Works with both source and `--il` output. Exits non-zero with
  `not found: no type matching '<NAME>'` / `no class matching '<NAME>'` if
  nothing matches.
- `--stdout`: print the matched type's source to stdout instead of writing
  files. For .NET requires `--type <NAME>` (exits non-zero with
  `usage: --stdout requires --type <NAME>` otherwise); for Java class files
  it works without `--type` since a class file holds exactly one class.
  Works with `--il`.
- `--json`, `--detect-obfuscation`, `--verify`: .NET-only; class files exit
  non-zero with `usage: --<flag> is not supported for Java class files`.

## Pipeline

### .NET

1. **PE parse** — DOS header → PE signature → COFF header → optional header
   (PE32 / PE32+) → data directories → section headers. Resolves the CLI
   runtime header (data directory 14) and converts RVAs to file offsets.
2. **CLI header** — `IMAGE_COR20_HEADER`: MetaData directory (rva at +8,
   size at +12), entry-point token.
3. **Metadata root** — `BSJB` signature, version string, stream headers.
   Streams captured: `#~`, `#Strings`, `#US`, `#GUID`, `#Blob`.
4. **Tables stream** — heap-sizes byte, `Valid` bitmask, per-table row counts,
   then packed rows. Index sizes: heap indexes (2/4 bytes from HeapSizes),
   table indexes (2/4 bytes from row counts), coded indexes (2/4 bytes from
   max referenced rows and tag-bit width).
5. **Signatures** — Type, MethodSig, FieldSig decoded from blob heap entries.
6. **CIL decode** — method bodies (tiny/fat headers) decoded into `Instruction`
   streams with typed operands.
7. **Emit** — IL disassembler or C# decompiler writes per-type output.

### Java

1. **Class file parse** — `0xCAFEBABE` magic → minor/major version →
   constant pool (17 tags; `Long`/`Double` take two slots; modified UTF-8) →
   access flags → this/super/interfaces → fields → methods → class
   attributes (`Code` with exception table + LocalVariableTable,
   `ConstantValue`, `Exceptions`, `InnerClasses`, `Signature`,
   `BootstrapMethods`, `SourceFile`).
2. **Descriptor parse** — JVMS 4.3 field/method descriptors to dotted Java
   type names.
3. **JVM bytecode decode** — `Instruction` streams with typed operands;
   `wide` expanded, `tableswitch`/`lookupswitch` decoded with padding,
   branch targets absolutized (offsets are relative to the opcode address).
4. **Emit** — javap-style disassembler or the Java decompiler writes output.

## Metadata tables

All ECMA-335 II.22 tables are sized via a generic column schema
(`schema(table) -> &[Col]`). Coded-index kinds (II.24.2.6) are decoded by
`decode_coded`. A coded value of `0` means "no reference".

## CIL opcodes

The opcode table (`cil/opcodes.rs`) is sourced from the dotnet/runtime
`opcode.def` master table (ECMA-335 III). Two-byte opcodes use the `0xFE`
prefix. Operand kinds: `None`, `SByte`, `Short`, `Int`, `Long`, `Float`,
`Double`, `BrTarget`, `ShortBrTarget`, `Switch`, `StringTok`, `FieldTok`,
`MethodTok`, `TypeTok`, `Tok`, `SigTok`, `ShortVar`, `Var`.

## C# decompiler

An expression-stack machine walks decoded instructions, maintaining a stack of
C# expression strings and emitting statements. Branch targets become
`Label_NNNN:` labels with `goto` transfers. Unsupported instructions emit a
`// unsupported:` comment and reset the stack, so output never aborts mid-method.

## Java decompiler

Same stack-machine architecture with Java-specific semantics:

- Stack values carry a semantic kind (`Num`/`Bool`/`Str`/`Ref`) so branch
  conditions render as `if (b)`, `if (i == 0)`, or `if (s == null)`.
- Generic `Signature` attributes (JVMS 4.7.9.1) drive type rendering:
  class/method type parameters (`Box<T>`, `<U>`), parameterized field and
  method types (`List<String>`, `Map<String, T>`), type variables and
  wildcards (`? extends X` / `? super X` / `?`). Method bodies stay erased.
- Values live across branches are flushed into per-target temps
  (`T{offset}_{i}`), declared with `var` at first assignment.
- javac 9+ string concatenation (`invokedynamic makeConcatWithConstants`) is
  reconstructed from the BootstrapMethods recipe; pre-9 StringBuilder chains
  are also folded to `+`.
- Catch variables are named lazily at handler entry; javac's finally rethrow
  and self-covering EH ranges are suppressed/collapsed.
- Post-passes reconstruct `for` loops (including `var` declarations),
  ternaries, and boolean returns (`return x > 0;`).
- Leftover `goto`/labels are commented out — Java has no `goto`.
- Enum constants, `values()`/`valueOf()`, `$VALUES`, bridge/synthetic
  members, and the implicit `extends Enum`/`Object`/`Record` are handled or
  suppressed per javac conventions.

## Error handling

`error::Error` covers IO, PE, metadata, CIL, class-file, and signature
failures plus a `NotImplemented` variant. The CLI prints `error: <message>`
and exits non-zero.

## Output format

One file per `TypeDef` (excluding the synthetic `<Module>`). File names are
`<Namespace>_<TypeName>.cs` (or `.il`), with generic-arity backticks and
nested separators sanitized to `_`. Java output is one `.java` file per
class (`<package>_<Class>.java`, or `<Class>.java` in the default package);
`--il` on a class file produces a `.jbc` disassembly file.
