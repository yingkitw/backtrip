# Agent Development Loop

This document defines the continuous improvement cycle for the **roundtrip** crate — a pure-Rust .NET decompiler that turns PE/CLI assemblies (`.dll`/`.exe`) back into readable C# (or IL).

## Project Structure

```
.
├── src/
│   ├── lib.rs          # crate root, module re-exports for integration tests
│   ├── main.rs         # clap CLI entry point (--il, --list, -o)
│   ├── error.rs        # Error + Result alias
│   ├── pe.rs           # PeImage: MZ/PE parse, COFF + optional header, data
│   │                   # directories, section headers, rva_to_offset, cli_header
│   ├── output.rs       # write_types — one file per DecompiledType
│   ├── cil/
│   │   ├── mod.rs      # module root
│   │   ├── opcodes.rs  # complete opcode → OpInfo lookup (1-byte + 0xFE 2-byte),
│   │   │               # sourced from dotnet/runtime opcode.def
│   │   ├── decoder.rs  # decode(&[u8]) -> Vec<Instruction> (offset, opcode,
│   │   │               # name, typed Operand, size)
│   │   └── disasm.rs   # IL disassembly text (IL_NNNN: op operand), token
│   │                   # resolution via Reader
│   ├── metadata/
│   │   ├── mod.rs      # module root
│   │   ├── streams.rs  # MetadataRoot: BSJB root, stream headers, heap accessors,
│   │   │               # #US UTF-16LE, decode_compressed_uint (ECMA-335)
│   │   ├── tables.rs   # table numbers (tbl), coded-index kinds (Coded),
│   │   │               # per-table column schemas (schema), IndexSizes,
│   │   │               # parse_tables, decode_coded
│   │   ├── signatures.rs # Type, MethodSig, ArrayShape, recursive parse_type /
│   │   │                 # parse_method_sig / parse_field_sig
│   │   └── reader.rs   # Reader: ergonomic accessors (names, sigs, bodies,
│   │                     # locals, generics, nested parents, C# type rendering)
│   └── decompile/
│       ├── mod.rs      # module root
│       └── csharp.rs   # decompile_assembly / _type / _method / _body — the
│                       # expression-stack machine (stack: Vec<String>,
│                       # out: Vec<String>, Label_NNNN + goto)
├── tests/
│   ├── integration.rs  # end-to-end tests against the Sample.dll fixture
│   └── fixtures/
│       ├── Sample.cs       # small C# library used as the test target
│       ├── sample.csproj   # .NET 8 project
│       └── bin/Release/net8.0/Sample.dll  # prebuilt fixture (rebuild with dotnet)
├── decompiled/         # sample decompiler output (Shapes_Calculator.cs, etc.)
├── Cargo.toml          # package metadata, deps (clap), release profile (lto)
└── Cargo.lock
```

## Build & test

```bash
cargo build          # compile
cargo test           # run integration + unit tests
cargo run -- <args>  # run the CLI
cargo clippy         # lint pass (warnings acceptable but noted)
```

The integration tests depend on a prebuilt .NET fixture at
`tests/fixtures/bin/Release/net8.0/Sample.dll`. Rebuild it with:

```bash
cd tests/fixtures && dotnet build -c Release
```

(dotnet SDK 8+ required.)

## The Loop

### 0. Consult MEMORY.md Before Starting
**CRITICAL**: Always begin each task by reading relevant sections of `MEMORY.md`. This prevents:
- Reinventing solutions to already-solved problems
- Repeating known mistakes and anti-patterns
- Missing established conventions for similar features
- Ignoring domain-specific pitfalls (RVA resolution, coded-index decoding, compressed-integer edge cases, stack-machine state corruption, opcode operand sizing)

**How to consult MEMORY.md**:
1. Search for keywords related to your task (e.g., "coded index", "fat method body", "stack machine", "OpInfo", "TypeDef")
2. Read relevant pattern sections for context and proven approaches
3. Follow established conventions unless there's a clear reason to diverge
4. If MEMORY.md lacks relevant patterns, note this during the harvest step (Step 4)

### 1. Complete Remaining TODO Items
Pick the next highest-priority item from `TODO.md` (or `ARCHITECTURE.md` if the task is architectural). If no high-priority items remain, run the competitive intelligence step to seed new work. Implement with minimal, focused changes. Do not add speculative features.

### 2. Create Tests and Examples
For every new capability:
- Add inline `#[cfg(test)] mod tests` in the relevant source file — exercise the feature end-to-end
- Add unit tests for core decoding/parsing logic
- Add or extend an integration test in `tests/integration.rs` that exercises the change through the real `Sample.dll` fixture
- If the feature adds a new opcode or decompilation path, ensure the fixture covers it (extend `Sample.cs` and rebuild if needed)

### 3. Ensure `cargo test` Passes
Run the full test suite:
```bash
cargo test                  # all inline unit tests + integration tests
cargo clippy                # lint pass (warnings acceptable but noted)
```
Fix any failures before proceeding. If the fixture needs rebuilding, run `cd tests/fixtures && dotnet build -c Release` first.

### 4. Harvest to MEMORY.md
After each completed feature, extract patterns and best practices:
- **Success patterns**: What worked well and should be repeated
- **Anti-patterns**: What to avoid in future implementations
- **CLI/ECMA-335 domain knowledge**: Opcode operand sizing, coded-index tag bits, compressed-integer decoding, fat-vs-tiny method bodies, RVA-to-offset resolution, signature blob grammar
- **Rust patterns**: roundtrip-specific conventions for the PE parser, metadata tables, CIL decoder, and the stack-machine decompiler
- **Testing patterns**: How to assert on decompiled C# fragments, fixture coverage gaps, regression cases for tricky opcodes

Add these to `MEMORY.md` with clear categories and references to specific files/lines.

**Harvest Quality Checklist**:
- [ ] Added success patterns with code examples where useful
- [ ] Documented anti-patterns with what to avoid instead
- [ ] Included domain-specific gotchas and edge cases (RVA math, coded indexes, stack state)
- [ ] Referenced specific files/lines for future lookup
- [ ] Used existing MEMORY.md categories or created new ones if needed
- [ ] Made entries searchable with relevant keywords
- [ ] Noted any gaps found in MEMORY.md during Step 0 consultation

### 5. Loop Back to Step 1
Return to `TODO.md` and pick the next item. Repeat until the backlog is clear.

### 6. Audit and Optimize
After each batch of features, perform a quality pass:
- **Maintainability**: Are functions small and well-named? Is the module structure logical?
- **Leanness**: Remove dead code, unused imports, and speculative abstractions
- **Wiring**: Ensure all new features are properly integrated into `lib.rs`, `main.rs` CLI args, and the decompile pipeline
- **Small footprint**: Avoid unnecessary dependencies; prefer standard library (currently only `clap`)
- **Consistency**: Match existing code style and patterns (Rust edition 2024)
- **Decompilation fidelity**: Verify output against the fixture and, where possible, against reference decompilers (ILSpy, dnSpy, ILSpy CLI) for correctness

### 7. Competitive Intelligence
Research similar .NET decompilers (ILSpy, dnSpy, dotPeek, JustDecompile, AvaloniaILSpy, `ilspycmd`, Cecil/mono.cecil for metadata). Identify capabilities they have that this project lacks — structured control-flow recovery, generics, async state machines, attribute emission, resource embedding, multi-assembly resolution, etc. Add the most valuable ones to the `TODO.md` brainstorming section. Prioritize features that provide clear competitive advantage.

### 8. Update Documentation
Keep all project docs aligned with the current implementation. Root docs (required):

- **`README.md`**: Quick start, CLI options, feature list, architecture summary
- **`ARCHITECTURE.md`**: Module relationships, data flow, design decisions
- **`TODO.md`**: Mark completed items, move them to Done, keep brainstorming current
- **`SPEC.md`**: CLI options, supported metadata tables, opcode coverage, output format
- **`MEMORY.md`**: Harvested patterns, domain knowledge, technical conventions

Update **`AGENTS.md`** (this file) if the loop itself evolves.

## Memory System (MEMORY.md)

### Purpose
`MEMORY.md` is the institutional knowledge repository that accelerates development by:
- **Preventing wheel reinvention**: Reuse proven patterns instead of guessing
- **Domain knowledge preservation**: Capture ECMA-335 / CIL rules that may be counter-intuitive (coded-index tag bits, compressed integers, fat-vs-tiny bodies, signature blob grammar)
- **Onboarding acceleration**: New contributors (human or AI) can understand patterns quickly
- **Quality consistency**: Ensure all features follow established conventions

### Structure
Organize `MEMORY.md` into these sections:

#### 1. PE & RVA Patterns
- MZ/PE header validation, PE32 vs PE32+ handling
- RVA-to-offset resolution across sections
- CLI header location and metadata directory discovery

#### 2. Metadata Table Patterns
- Generic column schema (`&[Col]`) and index-size computation (`IndexSizes`)
- Coded-index decoding (`decode_coded`, tag bits, "no reference" = 0)
- Compressed-integer decoding (ECMA-335 II.23.2)
- Heap accessors (`#Strings`, `#US`, `#GUID`, `#Blob`)

#### 3. Signature & Type Patterns
- `Type`, `MethodSig`, `ArrayShape` recursive decoders
- `parse_type` / `parse_method_sig` / `parse_field_sig` grammar
- C#-ish type-name rendering conventions

#### 4. CIL Decoder & Disasm Patterns
- `OpInfo` lookup (1-byte and `0xFE`-prefix 2-byte opcodes)
- Operand sizing per opcode (InlineI8, InlineR, InlineMethod, ShortInlineBrTarget, etc.)
- `Instruction` representation and `decode(&[u8])` conventions
- Disasm text rendering and token resolution via `Reader`

#### 5. Decompiler (Stack-Machine) Patterns
- `decompile_body` state: `stack: Vec<String>`, `out: Vec<String>`
- Branch target precomputation and `Label_NNNN:` / `goto` emission
- Unsupported opcodes: return `Ok(false)` to emit a comment and reset — never abort
- Control-flow reconstruction path (straight-line today, structured later)

#### 6. Testing Patterns
- Asserting on decompiled C# fragments in `tests/integration.rs`
- Fixture coverage: which opcodes/types `Sample.cs` exercises
- Rebuilding the fixture (`cd tests/fixtures && dotnet build -c Release`)
- Regression cases for tricky opcodes and edge cases

#### 7. CLI & Output Patterns
- `clap` arg handling (`--il`, `--list`, `-o`)
- `write_types` one-file-per-type output convention
- `lib.rs` re-exports for integration tests (no shelling out)

### Maintaining MEMORY.md Quality

**When to Update MEMORY.md**:
- After completing any non-trivial feature or bug fix
- When discovering a new pattern or anti-pattern
- After resolving a tricky debugging session (especially RVA math, coded-index decoding, stack-machine state)
- When establishing new conventions
- When an opcode's operand layout or a table's schema is confirmed against ECMA-335

**How to Write Good MEMORY.md Entries**:
1. **Be specific**: Reference actual files and line numbers where patterns occur
2. **Show examples**: Include minimal code snippets demonstrating the pattern
3. **Explain why**: Don't just say what—explain the reasoning behind the pattern
4. **Link related patterns**: Cross-reference related entries with `[[PatternName]]`
5. **Keep it searchable**: Use keywords that future developers will search for
6. **Date entries**: Add dates so readers know how current the information is

**Signs MEMORY.md Needs Attention**:
- Same questions or patterns coming up repeatedly in development
- Developers (human or AI) solving problems that should be documented
- Frequent bugs in similar areas (indicates missing anti-pattern documentation)
- New contributors asking the same questions
- Decompilation regressions in similar areas (e.g. repeated stack-state corruption)

**MEMORY.md Hygiene Routine** (run monthly):
- [ ] Review for outdated information (deprecated APIs, changed conventions)
- [ ] Consolidate redundant entries
- [ ] Add missing patterns from recent work
- [ ] Update cross-references if structure changed
- [ ] Verify all file references still exist
- [ ] Check for decompilation learnings that weren't captured

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

## Principles

- **Simplicity over flexibility**: Solve the problem at hand, not every hypothetical future problem
- **Surgical changes**: Touch only what you must; clean up only your own mess
- **Goal-driven**: Every change should have a verifiable success criterion
- **Test before ship**: No feature is complete until it has passing tests
- **Docs are code**: Documentation drift is a bug
- **Decompilation fidelity**: Output must reflect what the IL actually does; never emit plausible-looking but incorrect C#. When unsure, emit a comment or `goto` rather than guessing structure
- **Memory first**: Consult `MEMORY.md` before starting work—reuse proven patterns, avoid known pitfalls, follow established conventions. If MEMORY.md lacks relevant information, note the gap and fill it during harvest
- **Pattern harvesting**: After success, update `MEMORY.md` with patterns, anti-patterns, and learnings so others benefit from your experience
- **Memory hygiene**: Keep MEMORY.md current, searchable, and cross-referenced. It's only valuable if maintained

## File Positioning and Value

### README.md
- **Value**: User-facing documentation and project overview
- **Audience**: Users, contributors, stakeholders
- **Position**: Entry point for anyone discovering the project
- **Focus**: Features, quick start, CLI usage, architecture summary

### TODO.md
- **Value**: Feature roadmap and backlog management
- **Audience**: Development team (human and AI agents)
- **Position**: Development planning and prioritization
- **Focus**: What to build next, what's done, competitive intelligence

### ARCHITECTURE.md
- **Value**: Module relationships, data flow, and design decisions
- **Audience**: Contributors maintaining or extending the crate
- **Position**: Structural reference for the codebase
- **Focus**: Module boundaries, data flow (PE → metadata → CIL → C#), design decisions

### SPEC.md
- **Value**: Interface specification for the CLI and decompiler output
- **Audience**: Users and contributors integrating with roundtrip
- **Position**: Contract definition for inputs and outputs
- **Focus**: CLI options, supported metadata tables, opcode coverage, output format

### MEMORY.md
- **Value**: Institutional knowledge and pattern library
- **Audience**: Development team (accelerates onboarding and consistency)
- **Position**: Development acceleration and quality consistency
- **Focus**: Proven patterns, domain knowledge (ECMA-335/CIL), technical conventions
- **Update**: Must be updated after each completed feature to capture patterns and lessons learned

### AGENTS.md (this file)
- **Value**: Development process and workflow definition
- **Audience**: AI agents and human developers following the development loop
- **Position**: Process automation and continuous improvement
- **Focus**: How we work, the loop, memory system, principles
- **Update**: This file should be updated when the development loop itself evolves or when new process patterns emerge

## How These Files Work Together

1. **README.md** tells stakeholders what the project is and how to use it
2. **SPEC.md** defines the CLI and decompiler-output contract
3. **ARCHITECTURE.md** describes how the modules fit together (PE → metadata → CIL → C#)
4. **TODO.md** tells developers what to build next (driven by competitive intelligence)
5. **AGENTS.md** tells agents how to work through the TODO items with quality and memory
6. **MEMORY.md** captures what we learned so we don't repeat mistakes and provides proven patterns to accelerate development

The loop reinforces these files:
Consult MEMORY.md → Complete TODO → Test → Harvest to MEMORY → Optimize → Research → Update TODO

This creates a flywheel of continuous improvement with institutional knowledge preservation. **MEMORY.md is both the starting point (consult before work) and the destination (harvest after work), creating a virtuous cycle of learning and improvement.**
