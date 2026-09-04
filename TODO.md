# roundtrip — TODO

## Done

- [x] PE (PE32/PE32+) header parsing, section table, RVA resolution
- [x] CLI runtime header (IMAGE_COR20_HEADER) parsing
- [x] Metadata root + stream headers (#~, #Strings, #US, #GUID, #Blob)
- [x] Heap accessors: strings, blobs, user strings (UTF-16LE), GUIDs
- [x] Metadata tables: generic column schema, coded indexes, index sizing
- [x] Signature parsing: Type, MethodSig, FieldSig, LOCAL_SIG
- [x] CIL opcode table (ECMA-335 III, dotnet/runtime opcode.def)
- [x] CIL bytecode decoder (tiny/fat method bodies)
- [x] IL disassembler
- [x] C# decompiler: arithmetic, logic, comparison, conversions
- [x] C# decompiler: ldstr, ldc, args, locals, fields, arrays
- [x] C# decompiler: call/callvirt/newobj, box/unbox.any/castclass/isinst
- [x] C# decompiler: constructors, access modifiers, static/virtual/override
- [x] class/struct/interface/enum detection; generic type parameters
- [x] Per-type file output; CLI (clap) with --il / --list
- [x] Integration tests against a sample .NET assembly
- [x] Suppress redundant `base();` for constructors whose base is `object`
- [x] `--type <NAME>` to decompile a single type
- [x] `--stdout` to print a type to stdout instead of writing files
- [x] Better IL disassembly: resolve `ldstr` to the literal, locals names
- [x] `enum` underlying values (Constant table + Field defaults)
- [x] Nested types (NestedClass table → nested class output)
- [x] Static constants / `const` fields (Literal flag + Constant table)
- [x] Delegate types (`MulticastDelegate` base → `delegate` declaration)
- [x] Properties (PropertyMap / Property / MethodSemantics → getter/setter)
- [x] Events (EventMap / Event / MethodSemantics → add/remove/raise)
- [x] Custom attributes (CustomAttribute table → `[Attr(...)]`) — type-level, no args yet
- [x] Explicit interface implementations (MethodImpl)
- [x] P/Invoke (ImplMap / ModuleRef → `extern`)
- [x] Default parameter values (Param HasDefault flag + Constant table)
- [x] `switch` blocks (CIL `switch` → C# `switch`)
- [x] Custom attributes on methods/fields (not just types)
- [x] `try`/`catch`/`finally`/`fault` from exception regions (method section
      headers: EHCOR, fat sections)
- [x] Custom attribute constructor arguments (parse Value blob → `[Attr(...)]`)

## Next — control flow

- [x] Structured control-flow recovery: `if`/`else` from conditional branches
- [x] `while` loop reconstruction from back-edges
- [x] `do`-`while` loop reconstruction
- [x] `for` loop reconstruction (detect init + increment pattern)

## Next — language features

- [ ] `params` / `vararg` calling conventions

## Next — decompiler quality

- [ ] Type-name disambiguation: avoid collisions, emit `using` / full names
- [x] Remove redundant parentheses in expressions (precedence-aware printing)
- [ ] Collapse `dup`/`pop` patterns into temporaries cleanly
- [x] Reconstruct `ref`/`out`/`in` parameters from `ByRef` + modreq
- [x] Reconstruct `foreach` over `IEnumerable` from enumerator patterns
- [x] Reconstruct `using` blocks from `IDisposable` patterns
- [x] Reconstruct `lock` blocks from `Monitor.Enter/Exit`
- [x] Reconstruct auto-properties from compiler-generated backing fields
- [x] Reconstruct `string.Concat` chains back to `+` operators
- [x] Reconstruct collection initializers, object initializers

## Next — async / closures / state machines

- [ ] `async`/`await` state-machine recognition and reversal
- [ ] Lambda / closure reconstruction (display classes)
- [ ] `yield` iterator state-machine reversal
- [ ] `switch` expressions, pattern matching, `is` patterns

## Next — tooling

- [ ] Recursive multi-assembly decompilation (`--recursive` on a directory)
- [ ] Round-trip: recompile decompiled output and diff IL (verification)

## Brainstorming (competitive intelligence)

Compared to ILSpy / dnSpy / ILRepack / dotPeek / JetBrains dotPeek:

- ILSpy has high-quality C# reconstruction, decompilation of all C# features,
  and a tree UI. roundtrip should target the same output quality over time.
- dnSpy has a debugger. Out of scope for a CLI decompiler, but a `--watch`
  mode could be interesting.
- Iced (C# library) is a strong IL parser; roundtrip's Rust IL parser is
  comparable in coverage. Iced adds a full IL assembler — consider a
  round-trip assembler later.
- Add a `--json` output mode for machine consumption (type/method/field model).
- Add detection of obfuscation (encrypted strings, control-flow flattening)
  and emit warnings.
