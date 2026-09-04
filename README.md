# transcode

A .NET decompiler written in Rust. Parses ECMA-335 PE/CLI metadata and CIL
bytecode, then decompiles assemblies back to C# source (one file per type) or
to IL disassembly.

## Status

This is a working foundation, not a complete decompiler. It currently handles:

- PE (PE32 / PE32+) header parsing and RVA-to-offset resolution
- .NET metadata root + streams (`#~`, `#Strings`, `#US`, `#GUID`, `#Blob`)
- All standard metadata tables (generic, column-based schema with correct
  coded-index and heap-index sizing)
- Type, method, field, and parameter signatures (ECMA-335 II.23)
- Full CIL opcode table (ECMA-335 III) and bytecode decoder
- IL disassembler (ildasm-style output)
- C# decompiler for a meaningful subset:
  - arithmetic, logic, comparison, conversion operators
  - `ldstr`, `ldc.*`, arguments, locals
  - instance/static field access, array element access, `newarr`/`ldlen`
  - `call` / `callvirt` / `newobj` with argument reconstruction
  - `box` / `unbox.any` / `castclass` / `isinst`
  - constructors, access modifiers, `static` / `virtual` / `override` / `abstract`
  - class / struct / interface / enum kind detection
  - generic type parameters
  - control flow via `goto` labels (structured `if`/`for`/`while` is future work)

## Install

```bash
cargo build --release
```

## Usage

```bash
# Decompile to C# (one .cs file per type) into ./decompiled
transcode path/to/Assembly.dll

# Choose an output directory
transcode path/to/Assembly.dll -o out/

# Emit CIL disassembly instead of C#
transcode path/to/Assembly.dll --il

# List types only
transcode path/to/Assembly.dll --list
```

### Flags

| Flag            | Description                                  |
| --------------- | -------------------------------------------- |
| `<assembly>`    | Path to the .NET `.dll` / `.exe` (required)  |
| `-o, --output`  | Output directory (default `decompiled`)      |
| `--il`          | Emit IL disassembly instead of C# source     |
| `--list`        | List types in the assembly and exit          |
| `-h, --help`    | Show help                                    |
| `-V, --version` | Show version                                 |

## Project layout

```
src/
  pe.rs            PE parsing + RVA resolution
  metadata/
    streams.rs     metadata root + heaps (#Strings, #US, #GUID, #Blob)
    tables.rs      metadata table schemas + coded indexes + row parser
    signatures.rs  Type / MethodSig / FieldSig parsing
    reader.rs      high-level accessors tying everything together
  cil/
    opcodes.rs     full CIL opcode table
    decoder.rs     bytecode -> Instruction stream
    disasm.rs      Instruction -> IL text
  decompile/
    csharp.rs      C# decompiler (expression-stack machine)
  output.rs        per-type file writer
  main.rs          CLI (clap)
  lib.rs           library root (for integration tests)
tests/
  integration.rs   end-to-end tests against a sample .NET assembly
  fixtures/        sample C# project used as a test fixture
```

## Tests

```bash
cargo test
```

The integration tests build a small C# fixture (`tests/fixtures/Sample.cs`)
and assert the decompiler reproduces expected C# fragments.

## Limitations & roadmap

See `TODO.md`. Notable gaps: structured control-flow recovery (`if`/`for`/
`while` instead of `goto`), `try`/`catch`/`finally`, `async` state machines,
closures, pattern matching, properties/events, custom attribute rendering, and
fully qualified type-name disambiguation.

## License

MIT
