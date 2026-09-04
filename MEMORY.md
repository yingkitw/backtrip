# roundtrip — MEMORY.md

Institutional knowledge repository for the roundtrip .NET decompiler.
Consult before starting work (AGENTS.md Step 0); harvest to after (Step 4).

## 1. PE & RVA Patterns

_(No entries yet.)_

## 2. Metadata Table Patterns

_(No entries yet.)_

## 3. Signature & Type Patterns

### `strip_system` is applied to owner names in `resolve_method` (2026-09-04)
`resolve_method` in `src/decompile/csharp.rs` returns owner type names already
passed through `strip_system`, so `System.Object` becomes `Object`,
`System.ValueType` becomes `ValueType`, etc. When comparing an owner name
against a known framework type, compare against the stripped form
(`"Object"`, not `"System.Object"`).

## 4. CIL Decoder & Disasm Patterns

### `ldstr` token resolution in IL disasm (2026-09-04)
**Pattern**: `ldstr` operands are tokens with table id `0x70` (the user-string
token, indexing into the `#US` heap). `format_token` in
`src/cil/disasm.rs` checks `table == 0x70` *before* the metadata-table
`match` and resolves the string via `reader.root.get_user_string(row)`,
then quotes it with a local `quote_string` helper (ildasm-style escaping).

**Why before the match**: `0x70` is not a metadata table number (those are
0–44 per ECMA-335 II.22), so it would fall through to the
`[{table:#X}:{row}]` fallback. Handling it explicitly keeps the token
resolver self-contained.

**Gotcha**: `get_user_string` takes the *index* (the low 24 bits of the
token), not a row count. The `#US` heap stores length-prefixed blobs; the
last byte is a flag, not string data.

**Reference**: `src/cil/disasm.rs::format_token` (the `0x70` arm) and
`quote_string`.

### Local variable naming in IL disasm (2026-09-04)
**Pattern**: `ShortVar(i)` / `Var(i)` operands (used by `ldloc`/`stloc`/
`ldloca`) now render as `V_{i}` instead of a bare index, matching ildasm
conventions. The `.locals init (...)` header is emitted by `il_for_type`
in `src/main.rs` using `reader.local_types(body.local_token)` — each
local is rendered as `{type} V_{i}`.

**Why**: bare numbers are ambiguous (could be an arg index, a constant,
or a local); `V_0` is the ildasm convention and pairs with the `.locals`
header so the reader can cross-reference.

**Reference**: `src/cil/disasm.rs::format_operand` (`ShortVar`/`Var` arms),
`src/main.rs::il_for_type` (`.locals init` emission).

## 5. Decompiler (Stack-Machine) Patterns

### Suppress implicit `base()` to `System.Object` (2026-09-04)
**Pattern**: In the `call`/`callvirt` handler, when `mname == ".ctor"` and
`csig.has_this`, the call is a base-constructor invocation. The C# compiler
always emits a `call .ctor` on `System.Object` for any class without an
explicit base constructor call. Emitting `base();` for it produces redundant,
non-idiomatic C#.

**Fix**: check the resolved `owner` (already `strip_system`-ed — see
[[strip_system is applied to owner names in resolve_method]]). If
`owner == "Object"`, skip the `stmt(out, "base();")` emission but still
return `String::new()` so no expression is pushed and the `obj` (popped
receiver) is consumed.

**Why**: C# inserts the implicit `base()` call automatically; emitting it
produces output that doesn't round-trip to source users would write. Only
base constructors to *non-`object`* types should appear explicitly.

**Reference**: `src/decompile/csharp.rs`, `handle_instr` `call`/`callvirt`
arm, `.ctor` branch.

### Unsupported opcodes return `Ok(false)` — never abort (2026-09-04)
**Convention** (already established): `handle_instr` returns `Ok(false)` for
opcodes it cannot model. The caller in `decompile_body` then emits a
`// unsupported: <name> (<offset>)` comment and clears the stack. New opcode
handlers should follow this contract so decompilation never aborts mid-method.

**Reference**: `src/decompile/csharp.rs`, `decompile_body` loop and the
`_ => return Ok(false)` catch-all in `handle_instr`.

## 6. Testing Patterns

### Assert on decompiled C# fragments via the real fixture (2026-09-04)
Integration tests in `tests/integration.rs` load
`tests/fixtures/bin/Release/net8.0/Sample.dll`, run `decompile_assembly`, and
assert `source.contains(...)` / `!source.contains(...)` on the resulting C#.
This is the primary end-to-end verification path.

**Rebuild the fixture** when `Sample.cs` changes:
```bash
cd tests/fixtures && dotnet build -c Release
```
(dotnet SDK 8+ required.)

**Reference**: `tests/integration.rs::decompiles_constructor` (extended to
assert `!source.contains("base();")`).

### CLI smoke tests via `CARGO_BIN_EXE_roundtrip` (2026-09-04)
For testing CLI flags (`--stdout`, `--type`) that are pure plumbing over
already-tested library functions, shell out to the built binary using
`std::process::Command::new(PathBuf::from(env!("CARGO_BIN_EXE_roundtrip")))`.
Cargo builds the binary and sets this env var during integration tests.

**Pattern**:
- Assert `out.status.success()` and check `stdout` contains expected output.
- For error cases, assert `!out.status.success()` and check `stderr`
  contains the error message fragment.

**Why shell out**: `run()` is private to the binary crate and not accessible
from integration tests. The underlying library functions
(`decompile_type_by_name`, `decompile_il`) are already tested directly, so
CLI smoke tests only need to verify flag wiring and exit codes.

**Reference**: `tests/integration.rs::cli_stdout_prints_single_type`,
`cli_stdout_requires_type`.

## 7. CLI & Output Patterns

### `--type` flag and single-type decompile (2026-09-04)
**Pattern**: `decompile_type_by_name(reader, query) -> Result<Option<DecompiledType>>`
in `src/decompile/csharp.rs` matches a type by simple name (`Calculator`) OR
fully-qualified name (`Shapes.Calculator`). The CLI `--type <NAME>` flag
(`src/main.rs`) uses it for C# output; the IL path (`decompile_il`) accepts
an `Option<&str>` filter with the same matching rule. A non-match returns
`Error::NotFound` (C#) or an empty `Vec` (IL) → `Error::NotFound`.

**clap gotcha**: a field named `type_name` derives `--type-name`, not
`--type`. Override with `#[arg(long = "type", name = "type")]` to expose
`--type` while keeping the Rust identifier `type_name` (avoids the reserved
keyword `type`).

**Why**: matching on both simple and fully-qualified names is the least
surprising behavior for users who don't know the namespace; ILSpy/dnSpy
accept simple names in their navigation.

**Reference**: `src/decompile/csharp.rs::decompile_type_by_name`,
`src/main.rs::run` and `decompile_il`.

### `Error::NotFound` variant (2026-09-04)
Added `Error::NotFound(String)` to `src/error.rs` for "no type matching"
cases. Prefer this over misusing `NotImplemented` (which takes
`&'static str` and is semantically about unsupported features, not missing
lookups). Display: `not found: <message>`.

### `Error::Usage` variant (2026-09-04)
Added `Error::Usage(String)` to `src/error.rs` for CLI argument errors
(e.g. `--stdout` without `--type`). Display: `usage: <message>`. Prefer
this over misusing `InvalidPe`/`NotImplemented` for user-facing CLI
constraint violations.

### `--stdout` flag (2026-09-04)
**Pattern**: `--stdout` prints the matched type's source to stdout via
`print!("{}", t.source)` (no trailing newline added — the source already
ends with one). It requires `--type <NAME>`; the guard is in `run()` and
returns `Error::Usage`. Works for both C# and `--il` paths.

**Why require `--type`**: printing all types to stdout would produce a
jumbled, unparseable stream. Single-type stdout is the useful case (quick
inspection, piping into other tools).

**Reference**: `src/main.rs::run`, `--stdout` branches in both `--il` and
C# paths.
