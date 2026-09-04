# roundtrip — Specification

## Purpose

Decompile .NET assemblies (PE/CLI, ECMA-335) into C# source or CIL disassembly.

## Inputs

A single PE file (`.dll` or `.exe`) containing a .NET CLI runtime header
(PE data directory entry 14). Non-managed PE files are rejected with
`invalid PE: no CLI runtime header; not a managed .NET assembly`.

## CLI

```
roundtrip <ASSEMBLY> [-o <DIR>] [--il] [--list] [--type <NAME>] [--stdout]
```

- Default action: decompile to C#, one `.cs` file per type, written to the
  output directory (default `decompiled`).
- `--il`: write `.il` disassembly files instead.
- `--list`: print fully-qualified type names to stdout and exit.
- `--type <NAME>`: decompile only the type whose simple name
  (`Calculator`) or fully-qualified name (`Shapes.Calculator`) matches.
  Works with both C# and `--il` output. Exits non-zero with
  `not found: no type matching '<NAME>'` if no type matches.
- `--stdout`: print the matched type's source to stdout instead of writing
  files. Requires `--type <NAME>` (exits non-zero with
  `usage: --stdout requires --type <NAME>` otherwise). Works with `--il`.

## Pipeline

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

## Error handling

`error::Error` covers IO, PE, metadata, CIL, and signature failures plus a
`NotImplemented` variant. The CLI prints `error: <message>` and exits non-zero.

## Output format

One file per `TypeDef` (excluding the synthetic `<Module>`). File names are
`<Namespace>_<TypeName>.cs` (or `.il`), with generic-arity backticks and
nested separators sanitized to `_`.
