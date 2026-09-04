# backtrip — .NET Decompiler & CIL Disassembler in Rust

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust Edition](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org/)
[![Build](https://img.shields.io/badge/build-cargo%20test-green.svg)](#tests)

**backtrip** is a .NET decompiler and CIL disassembler written in Rust. It
parses ECMA-335 PE/CLI metadata and CIL bytecode, then reconstructs readable
C# source code from compiled .NET assemblies (`.dll` / `.exe`). It also
provides IL disassembly, obfuscation detection, structural verification, and
JSON metadata export — all from a single fast native binary with zero runtime
dependencies.

## Features

### .NET Metadata & IL Parsing

- **PE32 / PE32+ header parsing** with RVA-to-offset resolution
- **Full metadata stream parsing** — `#~`, `#Strings`, `#US`, `#GUID`, `#Blob`
- **All standard metadata tables** via a generic, column-based schema with
  correct coded-index and heap-index sizing
- **Type, method, field, and parameter signatures** (ECMA-335 II.23)
- **Complete CIL opcode table** (ECMA-335 III) and bytecode decoder
- **IL disassembler** producing ildasm-style output

### C# Decompilation

- **Arithmetic, logic, comparison, and conversion operators**
- **String literals, numeric constants, arguments, and locals**
- **Instance/static field access, array element access, `newarr`/`ldlen`**
- **Method calls** — `call` / `callvirt` / `newobj` with argument reconstruction
- **Type operations** — `box` / `unbox.any` / `castclass` / `isinst` (`is`/`as`)
- **Constructors** with access modifiers, `static` / `virtual` / `override` / `abstract`
- **Type kinds** — class / struct / interface / enum / delegate detection
- **Generic type parameters**
- **Properties and events** with getter/setter rendering
- **Custom attributes** (`[Obsolete]`, `[DllImport]`, etc.)

### Structured Control Flow Recovery

- **`if` / `else if` / `else`** — reconstructed from conditional branches
- **`while` and `do-while` loops** — reconstructed from backward branches
- **`for` loops** — detected from initializer + condition + incrementer patterns
- **`switch` statements** — case bodies inlined, `goto` labels eliminated
- **`switch` expressions** — `V_0 = ...; goto` pattern → clean `switch` with `break`
- **`try` / `catch` / `finally`** — reconstructed from exception handlers
- **`using` blocks** — detected from `try`/`finally` + `Dispose()` pattern
- **`foreach` loops** — detected from `GetEnumerator` + `MoveNext` + `Current` pattern
- **Collection initializers** — `new List()` + `.Add()` calls collapsed to `{ ... }`

### Tooling & Analysis

- **Recursive multi-assembly decompilation** — `--recursive` decompiles all
  `.dll`/`.exe` files in a directory
- **JSON metadata export** — `--json` outputs a structured model of types,
  methods, and fields for machine consumption
- **Obfuscation detection** — `--detect-obfuscation` scans for non-printable
  names, control-flow flattening, and string encryption
- **Structural verification** — `--verify` checks that all types/methods/fields
  from metadata appear in the decompiled output

## Install

```bash
cargo build --release
```

The resulting binary is at `target/release/backtrip`. No .NET runtime is
required to run backtrip — it parses PE files directly.

## Usage

```bash
# Decompile to C# (one .cs file per type) into ./decompiled
backtrip path/to/Assembly.dll

# Choose an output directory
backtrip path/to/Assembly.dll -o out/

# Decompile a single type to stdout
backtrip path/to/Assembly.dll --type Calculator --stdout

# Emit CIL disassembly instead of C#
backtrip path/to/Assembly.dll --il

# List types in the assembly
backtrip path/to/Assembly.dll --list

# Recursively decompile all assemblies in a directory
backtrip ./bin/Release/net8.0/ --recursive -o out/

# Export assembly metadata as JSON
backtrip path/to/Assembly.dll --json

# Detect obfuscation indicators
backtrip path/to/Assembly.dll --detect-obfuscation

# Verify decompiled output against metadata
backtrip path/to/Assembly.dll --verify
```

### CLI Reference

| Flag                    | Description                                                  |
| ----------------------- | ------------------------------------------------------------ |
| `<assembly>`            | Path to .NET `.dll` / `.exe`, or a directory with `--recursive` |
| `-o, --output <dir>`    | Output directory (default `decompiled`)                      |
| `--il`                  | Emit IL disassembly instead of C# source                     |
| `--list`                | List types in the assembly and exit                          |
| `--type <name>`         | Decompile only the matching type                             |
| `--stdout`              | Print matched type to stdout (requires `--type`)             |
| `--recursive`           | Decompile all `.dll`/`.exe` in a directory                   |
| `--json`                | Export assembly metadata as JSON to stdout                   |
| `--detect-obfuscation`  | Scan for obfuscation indicators and emit warnings             |
| `--verify`              | Verify decompiled output against metadata                    |
| `-h, --help`            | Show help                                                    |
| `-V, --version`         | Show version                                                 |

## Project Layout

```
src/
  pe.rs               PE parsing + RVA resolution
  metadata/
    streams.rs        metadata root + heaps (#Strings, #US, #GUID, #Blob)
    tables.rs         metadata table schemas + coded indexes + row parser
    signatures.rs     Type / MethodSig / FieldSig parsing
    reader.rs         high-level accessors tying everything together
  cil/
    opcodes.rs        full CIL opcode table (ECMA-335 III)
    decoder.rs        bytecode → Instruction stream
    disasm.rs         Instruction → IL text
  decompile/
    csharp.rs         C# decompiler (expression-stack machine)
    json.rs           JSON metadata export
    obfuscation.rs    obfuscation detection
    verify.rs         structural verification
  output.rs           per-type file writer
  main.rs             CLI (clap)
  lib.rs              library root (for integration tests)
tests/
  integration.rs      end-to-end tests against a sample .NET assembly
  fixtures/           sample C# project used as a test fixture
```

## Tests

```bash
cargo test
```

The integration tests build a small C# fixture (`tests/fixtures/Sample.cs`)
using the .NET SDK (version 8+) and assert the decompiler reproduces expected
C# fragments — including control flow, switch statements, switch expressions,
try/catch, using blocks, foreach, properties, events, delegates, nested types,
enums, interfaces, abstract/virtual/override hierarchies, generic classes and
methods, static constructors, arrays, and box/unbox/castclass conversions.

## Library Usage

backtrip is a library as well as a CLI. The
[`examples/decompile.rs`](examples/decompile.rs) example shows the minimal
pipeline — parse PE, load metadata, decompile:

```bash
cargo run --example decompile -- path/to/Assembly.dll
```

The same functions (`PeImage::parse`, `metadata::load`, `Reader::new`,
`decompile_assembly`) power the CLI and the integration tests.

## How It Works

backtrip works in three stages:

1. **PE parsing** — Reads the portable executable headers and locates the
   .NET metadata root and CLI header.
2. **Metadata decoding** — Parses the metadata streams and tables to extract
   type definitions, method signatures, field layouts, and custom attributes.
3. **CIL decompilation** — Decodes the IL bytecode into an instruction stream,
   then uses an expression-stack machine to reconstruct C# source. Post-
   processing passes restructure control flow (`if`/`else`, `while`, `for`,
   `switch`, `try`/`catch`), collapse patterns (`using`, `foreach`,
   collection initializers), and clean up compiler-generated names.

## Comparison with Other Tools

| Feature | backtrip | ILSpy | dnSpy | dotPeek |
| ------- | --------- | ----- | ----- | ------- |
| Language | Rust | C# | C# | C# |
| CLI tool | Yes | Yes | No (GUI) | No (GUI) |
| IL disassembly | Yes | Yes | Yes | Yes |
| C# decompilation | Partial | Full | Full | Full |
| JSON export | Yes | No | No | No |
| Obfuscation detection | Yes | No | No | No |
| Structural verification | Yes | No | No | No |
| Recursive batch mode | Yes | Yes | No | No |
| .NET runtime required | No | Yes | Yes | Yes |

backtrip differentiates itself by being a lightweight, dependency-free native
binary with unique analysis features (obfuscation detection, verification,
JSON export) that complement traditional decompilers.

## Limitations & Roadmap

See [`TODO.md`](TODO.md) for the full roadmap. Notable upcoming work:

- `async` / `await` state-machine recognition and reversal
- `yield` iterator state-machine reversal
- Lambda / closure full inlining (display class → lambda expression)
- Switch expressions with type patterns and property patterns
- Object initializers (`new T() { Prop = v }` collapse)
- Generic parameters rendered by name (`T`/`U`) instead of index-based `T0`/`!!0`
- Full round-trip verification (recompile + IL diff)

## License

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
