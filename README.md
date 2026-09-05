# backtrip — .NET Decompiler & CIL Disassembler in Rust

[![crates.io](https://img.shields.io/crates/v/backtrip.svg)](https://crates.io/crates/backtrip)
[![docs.rs](https://docs.rs/backtrip/badge.svg)](https://docs.rs/backtrip)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust Edition](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org/)
[![Build](https://img.shields.io/badge/build-cargo%20test-green.svg)](#tests)

**backtrip** is a .NET decompiler and CIL disassembler written in Rust. It
parses ECMA-335 PE/CLI metadata and CIL bytecode, then reconstructs readable
C# source code from compiled .NET assemblies (`.dll` / `.exe`). It also
provides IL disassembly, obfuscation detection, structural verification, and
JSON metadata export — all from a single fast native binary with zero runtime
dependencies.

## Why backtrip?

Most .NET decompilers are large, C#-based tools that require a .NET runtime
and, in the common case, a desktop GUI (ILSpy, dnSpy, dotPeek). backtrip
deliberately takes the opposite trade-off:

- **One native binary, zero runtime dependencies** — written in Rust, it
  parses PE/CLI metadata and CIL directly. No .NET runtime, no JVM, no GUI:
  it runs anywhere Cargo does and finishes in milliseconds.
- **Scriptable and composable** — a real CLI (`--json`, `--list`, `--type`,
  `--recursive`) plus a small library API. Decompilation, IL disassembly, and
  metadata extraction pipe cleanly into diffs, CI pipelines, and custom
  analysis tools.
- **Verifiable fidelity** — the round-trip test decompiles a real assembly,
  recompiles the emitted C# with `dotnet`, and re-decompiles the result: the
  output must build with zero errors and reach a fixed point (identical
  output up to label renumbering). When the decompiler cannot confidently
  reconstruct control flow, it emits `goto` labels or a comment rather than
  plausible-looking but incorrect C#.
- **Analysis built in** — obfuscation detection, structural verification
  (`--verify`), and JSON export are first-class features, not afterthoughts.
- **Lightweight by design** — one dependency (`clap`), a small footprint, and
  fast incremental builds.

**When to reach for backtrip**: quick inspection on any machine; batch or
recursive analysis of many assemblies; CI gates that must prove metadata
survives decompilation; machine-readable extraction via JSON; headless
pipelines. **When to reach for ILSpy / dnSpy / dotPeek**: interactive
browsing, debugger-integrated inspection, or when you need the last few
percent of C# reconstruction — `async`/`await` and `yield` state machines and
full pattern matching are not there yet (see
[Limitations](#limitations--roadmap)).

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
- **`ref` / `out` parameters** — ByRef params render with correct semantics and call-site keywords
- **`params` arrays** — `[ParamArray]` renders as `params int[] xs`; vararg signatures render their sentinel as `...`
- **Lambda inlining** — display class + `Func`/`Action` construction + `Invoke` collapse to a lambda expression with captured variables (`((int x) => x + offset)(10)`)
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
- **Object initializers** — `new T() { Prop = v }` collapsed from `new`/`default` + member sets

### Tooling & Analysis

- **Compilable output** — `using` directives inferred from the assembly's
  type references (`System`, `System.IO`, ...), `System.*` names rendered as
  simple names, and correct statement forms (ref/out params, property
  accessors, foreach variables, bool literals, `base()` initializers) so the
  decompiled C# rebuilds under `dotnet` — validated by the round-trip test
- **Recursive multi-assembly decompilation** — `--recursive` decompiles all
  `.dll`/`.exe` files in a directory
- **JSON metadata export** — `--json` outputs a structured model of types,
  methods, and fields for machine consumption
- **Obfuscation detection** — `--detect-obfuscation` scans for non-printable
  names, control-flow flattening, and string encryption
- **Structural verification** — `--verify` checks that all types/methods/fields
  from metadata appear in the decompiled output

## Install

From [crates.io](https://crates.io/crates/backtrip):

```bash
cargo install backtrip
```

Or build from source:

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
examples/
  decompile.rs        library-usage example (parse → decompile → print)
tests/
  integration.rs      end-to-end tests against the Sample.dll fixture
  fixtures/           Sample.cs (fragment assertions)
  fixtures/roundtrip/ Roundtrip.cs (round-trip compile test fixture)
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

A second fixture (`tests/fixtures/roundtrip/Roundtrip.cs`) drives the
**round-trip test**: the decompiled output is recompiled with `dotnet` and
must build (0 errors), then the recompiled assembly is re-decompiled and must
expose the same type set. This validates that the emitted C# is actually
compilable — usings, name resolution, and statement rendering.

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

| Feature | backtrip | ildasm | ILSpy | dnSpy | dotPeek |
| ------- | --------- | ------ | ----- | ----- | ------- |
| Language | Rust | C++/CLI | C# | C# | C# |
| License | Apache-2.0 | MS | MIT | MIT | Proprietary (free) |
| CLI tool | Yes | Yes | Yes (`ilspycmd`) | No (GUI) | No (GUI) |
| IL disassembly | Yes | Yes | Yes | Yes | Yes |
| C# decompilation | Partial | No | Full | Full | Full |
| Zero runtime dependencies | Yes | No (SDK) | No (.NET) | No (.NET) | No (.NET) |
| Round-trip compile gate | Yes | No | No | No | No |
| JSON export | Yes | No | No | No | No |
| Obfuscation detection | Yes | No | No | No | No |
| Structural verification | Yes | No | No | No | No |
| Recursive batch mode | Yes | No | Yes | No | No |

**Where each tool fits**:

- **ildasm** — the reference IL disassembler shipped with the .NET SDK.
  IL only; no C# reconstruction. Good for checking what the compiler
  actually emitted.
- **ILSpy / ilspycmd** — the open-source standard for C# decompilation.
  Excellent output quality, actively developed, usable as a library and a
  CLI. The benchmark to beat for reconstruction fidelity.
- **dnSpy** — ILSpy-derived; adds a debugger and assembly editing. GUI-only.
- **dotPeek** — JetBrains' free decompiler. Polished GUI plus PDB
  generation; Windows-centric.
- **backtrip** — a headless, dependency-free alternative with verification
  tooling the GUI tools lack. Choose it where a native binary, scripting, or
  CI integration matters more than maximum C# fidelity — and use the
  round-trip gate to *prove* the output compiles.

## Limitations & Roadmap

See [`TODO.md`](TODO.md) for the full roadmap. Notable upcoming work:

- `async` / `await` state-machine recognition and reversal
- `yield` iterator state-machine reversal
- Switch expressions with type patterns and property patterns
- Round-trip IL diffing (recompiled IL never matches exactly; semantic
  comparison is future work)

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
