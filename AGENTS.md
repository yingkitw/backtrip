# AGENTS.md

Development guide for agents working on transcode.

## Build & test

```bash
cargo build          # compile
cargo test           # run integration + unit tests
cargo run -- <args>  # run the CLI
```

The integration tests depend on a prebuilt .NET fixture at
`tests/fixtures/bin/Release/net8.0/Sample.dll`. Rebuild it with:

```bash
cd tests/fixtures && dotnet build -c Release
```

(dotnet SDK 8+ required.)

## Development loop

1. Pick the next item from `TODO.md`.
2. Implement it with minimal, focused changes.
3. Add or extend tests in `tests/integration.rs` (and unit tests where
   appropriate) that exercise the change end-to-end.
4. Ensure `cargo test` passes.
5. Update `README.md` / `SPEC.md` / `ARCHITECTURE.md` / `TODO.md` to stay
   aligned with the implementation.

## Conventions

- Rust edition 2024. Keep dependencies minimal (currently only `clap`).
- Match existing module boundaries; do not introduce speculative abstractions.
- The metadata table parser uses a generic column schema — add new tables by
  extending `schema()` in `src/metadata/tables.rs`.
- The C# decompiler is an expression-stack machine in
  `src/decompile/csharp.rs`. Add opcode handling in `handle_instr`; unsupported
  opcodes should return `Ok(false)` so they emit a comment instead of aborting.
- CIL opcode values must match ECMA-335 III / dotnet/runtime `opcode.def`.

## Reference specs

- ECMA-335 (CLI) — Partition II (metadata), Partition III (CIL).
- dotnet/runtime `src/coreclr/inc/opcode.def` — authoritative opcode table.
