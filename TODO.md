# backtrip — TODO

**Test suite**: 45 inline unit tests; 70 integration tests
(including `tests/docs_sync.rs`, which fails `cargo test` when these
counts or the AGENTS.md module list go stale — update the DOC, not the test).

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
- [x] Shared control-flow engine (`decompile/flow.rs`) used by both back ends

## Done — Java class files

- [x] Class file parser (JVMS 4): constant pool (17 tags, modified UTF-8),
      access flags, fields, methods, `Code` + exception table,
      `ConstantValue`, `Exceptions`, `InnerClasses`, `Signature`,
      `LocalVariableTable`, `BootstrapMethods`, `SourceFile`
- [x] Field/method descriptor parsing (JVMS 4.3) incl. arrays + generics
      type params
- [x] JVM opcode table (JVMS 6, 0x00–0xC9) + bytecode decoder (`wide`,
      `tableswitch`/`lookupswitch` with padding, absolute branch targets)
- [x] javap-style disassembler (`--il` on `.class` → `.jbc` files)
- [x] Java source decompiler: classes/interfaces/enums/@interfaces,
      modifiers, constants, static initializers, enum constants (values/
      valueOf/$VALUES/synthetic suppressed)
- [x] Expression-stack machine: arithmetic/bitwise/shifts with precedence,
      casts, array load/store, fields, static+virtual+interface calls,
      super/this ctor chaining, `new`/`newarray`/`multianewarray`,
      `checkcast`/`instanceof`, `iinc` → `+=`/`-=`
- [x] Cross-block stack values via per-target join temps (verifier-consistent)
- [x] javac string concat: `invokedynamic makeConcatWithConstants` recipe →
      `+` expressions; pre-9 StringBuilder chains also folded
- [x] Control flow: javac-shape while loops, for loops (with `var` init),
      do-while, switch inlining, ternary + boolean-return reconstruction
- [x] `try`/`catch`/`finally` from the exception table (handler-region
      boundaries, self-covering ranges, lazy catch-var naming, finally
      rethrow suppression)
- [x] CLI: magic-byte detection (`MZ` vs `0xCAFEBABE`), `--list`, `--il`,
      `--type`, `--stdout`, `.class` in `--recursive`
- [x] Integration tests + fixture (`tests/fixtures/java/`, javac 21) incl.
      decompiled output recompiled with `javac`

## Next — control flow

- [x] Structured control-flow recovery: `if`/`else` from conditional branches
- [x] `while` loop reconstruction from back-edges
- [x] `do`-`while` loop reconstruction
- [x] `for` loop reconstruction (detect init + increment pattern)

## Next — language features

- [x] `params` (ParamArray attribute → `params int[] xs` keyword)
- [x] `vararg` signature parsing (SENTINEL → `...`; C# cannot declare vararg
      methods, so covered by a unit test, not the fixture)

## Next — decompiler quality

- [x] Type-name disambiguation: avoid collisions, emit `using` / full names
- [x] Remove redundant parentheses in expressions (precedence-aware printing)
- [x] Collapse `dup`/`pop` patterns into temporaries cleanly
- [x] Reconstruct `ref`/`out`/`in` parameters from `ByRef` + modreq
- [x] Reconstruct `foreach` over `IEnumerable` from enumerator patterns
- [x] Reconstruct `using` blocks from `IDisposable` patterns
- [x] Reconstruct `lock` blocks from `Monitor.Enter/Exit`
- [x] Reconstruct auto-properties from compiler-generated backing fields
- [x] Reconstruct `string.Concat` chains back to `+` operators
- [x] Reconstruct collection initializers (`new List() { ... }`)
- [x] Reconstruct object initializers (`new T() { Prop = v }` collapse;
      struct case via `default(T)` + member sets)
- [x] Render `static` on static field declarations
- [x] Strip generic arity backticks from class declarations (`Box`1` → `Box`)
- [x] Render generic parameters by name (`T`/`U`) instead of index-based
      `T0`/`!!0` — `Reader::type_name_ctx` with class/method GenericParam names
- [x] Strip namespace prefixes from same-namespace base classes
      (`Circle : Shapes.Shape` → `Circle : Shape`) — cross-namespace bases
      keep their qualified name

## Next — async / closures / state machines

- [ ] `async`/`await` state-machine recognition and reversal
- [x] Lambda / closure reconstruction (display class name cleanup)
- [x] Lambda / closure inlining (display class → lambda expression) — direct
      `new Func/Action<T...>(ctx, DisplayClass.lambda_N).Invoke(args)` becomes
      `((T x) => body)(args)` with captured fields substituted. Block-bodied
      lambdas, stored delegates, and multi-lambda display classes are left
      as-is (safe degradation)
- [ ] `yield` iterator state-machine reversal
- [x] `is` patterns and `as` operator rendering (isinst/castclass cleanup)
- [x] `switch` statement reconstruction (inlined case bodies + default)
- [x] `switch` expressions (goto+assign pattern → switch with break + return)
- [ ] `switch` expressions with pattern matching (type patterns, property patterns)

## Next — tooling

- [x] Recursive multi-assembly decompilation (`--recursive` on a directory)
- [x] Structural verification (`--verify`): check all types/methods/fields appear in output
- [x] Round-trip test: decompile `tests/fixtures/roundtrip/Roundtrip.dll` →
      recompile the decompiled C# with `dotnet` → re-decompile the result.
      The output must BUILD (usings, name resolution, statement rendering)
      and the recompiled assembly must expose the same type set. IL diffing
      remains future work (recompiled IL never matches exactly).

## Next — Java (future)

- [x] `.jar` archive support (zip central directory + built-in raw-DEFLATE
      decoder — stored and deflated entries; `--list`/`--type`/`--il`;
      broken entries skipped gracefully; demo.jar fixtures)
- [x] Generic `Signature` rendering in declarations (JVMS 4.7.9.1):
      class/method type params (`Box<T>`, `<U>`), parameterized types
      (`List<String>`, `Map<String, T>`), type variables and wildcards —
      `parse_type_signature` / `parse_method_signature`; Box.java fixture
      round-trips through javac
- [x] `synchronized` block reconstruction (monitor enter/exit + EH ranges;
      stash-store suppression, sync-handler detection, javac round-trip via
      the Sync.java fixture)
- [ ] Lambda / method-reference reconstruction (`LambdaMetafactory`)
- [ ] Local variable table (`-g`) naming end-to-end in bodies
- [ ] Stack-map-aware type inference: bodies/`new`/foreach render generics
      (`new java.util.HashMap<>()`, `for (T t : items)` from the iterator
      pattern) and `var` declarations

## Brainstorming (competitive intelligence)

Compared to ILSpy / dnSpy / ILRepack / dotPeek / JetBrains dotPeek:

- ILSpy has high-quality C# reconstruction, decompilation of all C# features,
  and a tree UI. backtrip should target the same output quality over time.
- dnSpy has a debugger. Out of scope for a CLI decompiler, but a `--watch`
  mode could be interesting.
- Iced (C# library) is a strong IL parser; backtrip's Rust IL parser is
  comparable in coverage. Iced adds a full IL assembler — consider a
  round-trip assembler later.
- [x] Add a `--json` output mode for machine consumption (type/method/field model).
- [x] Add detection of obfuscation (encrypted strings, control-flow flattening)
  and emit warnings.

Compared to Java decompilers (CFR / Vineflower / Procyon / Fernflower):

- CFR/Vineflower render method bodies with full generic type inference and
  emit `import` lists instead of fully-qualified names — backtrip renders
  FQNs today (except `java.lang`). Import generation would shrink output
  substantially and is the single biggest readability gap.
- Vineflower reconstructs Java foreach from the iterator pattern; backtrip
  renders a while loop today (see Stack-map-aware inference TODO).
- Both reconstruct `synchronized` blocks and lambdas from
  LambdaMetafactory bootstraps; backtrip comments these out (TODO above).

## Scorecard

**2026-09-23** — /30, evidence-based (AGENTS.md Step 7):

| Dimension | Score | Evidence |
|---|---|---|
| Correctness & coverage | 4 | 99 tests green (34 unit + 65 integration incl. javac round-trip); no coverage tooling run yet |
| Decompilation fidelity | 4 | Sample/Box decompiled output recompiles (javac) and Roundtrip.dll fixed-points (dotnet); method bodies still erased/raw |
| Maintainability | 3 | ~94 clippy warnings (pre-existing patterns), `csharp.rs` 3.1k lines |
| Docs alignment | 5 | `tests/docs_sync.rs` enforces module list + test counts + ARCHITECTURE coverage |
| Wiring & ergonomics | 4 | CLI complete for both formats; shared `flow.rs`; magic-byte dispatch |
| Footprint | 5 | Single dependency (`clap`), no speculative abstractions |

**Weakest dimension: Maintainability (3).** Action: burn down the clippy
warning count (mostly carried-over `needless_range_loop`/`collapsible_if`
patterns in `csharp.rs`/`java.rs`) and split oversized `decompile_body`
functions. Mature-module rules apply — behavior-preserving, covered by the
existing suite.

**2026-09-23 (follow-up)** — clippy burn-down executed: **94 → 0 warnings**,
C# fixture output verified byte-identical. Re-scored:

| Dimension | Score | Evidence |
|---|---|---|
| Correctness & coverage | 4 | unchanged — 99 tests green |
| Decompilation fidelity | 4 | unchanged — fixture outputs identical |
| Maintainability | **4** | 0 clippy warnings; shared `indent_range`/flow helpers; oversized-function split still open (java.rs ~1.9k, csharp.rs ~3.1k lines) |
| Docs alignment | 5 | enforced by docs_sync |
| Wiring & ergonomics | 4 | unchanged |
| Footprint | 5 | unchanged |

**25 → 26/30.** Next weakest: decompilation fidelity (4) — generic type
inference in method bodies (stack-map frames) is the concrete lever.

**2026-09-23 (iteration 3)** — fidelity lever partially executed: enhanced-for
reconstruction (`for (var V_4 : this.items)`) from the lowered iterator
pattern, while-condition double-paren fix (`unwrap_parens`), 4 new unit
tests. Re-scored:

| Dimension | Score | Evidence |
|---|---|---|
| Correctness & coverage | 4 | 103 tests green |
| Decompilation fidelity | 4→4 (progress) | foreach + paren fix landed; body generics still erased (stack-map inference open) |
| Maintainability | 4 | 0 clippy warnings maintained |
| Docs alignment | 5 | docs_sync caught 2 more count drifts this session |
| Wiring & ergonomics | 4 | unchanged |
| Footprint | 5 | unchanged |

**26/30.** Next lever unchanged: stack-map-aware generic inference in method
bodies (`new HashMap<>()`, typed foreach elements).

**2026-09-23 (iteration 4)** — `synchronized` block reconstruction landed
(TODO ✓), plus global label/goto-noise cleanup (`remove_unreferenced_labels`
+ `remove_redundant_jump_pairs`): Sample.class output is now free of comment
noise entirely. Re-scored:

| Dimension | Score | Evidence |
|---|---|---|
| Correctness & coverage | 4 | 104 tests green |
| Decompilation fidelity | 4→5 | Sync/Sample/Box all recompile; no EH/monitor/label noise; bodies still erased (only remaining gap) |
| Maintainability | 4 | 0 clippy warnings |
| Docs alignment | 5 | docs_sync enforced |
| Wiring & ergonomics | 4 | unchanged |
| Footprint | 5 | unchanged |

**27/30.** Remaining levers are large: body-level generic inference, `.jar`
support, lambda reconstruction.

**2026-09-25 (iteration 5)** — `.jar` support landed (TODO ✓): zero-dependency
zip reader + raw-DEFLATE decoder, CLI parity with class files, graceful
degradation on broken entries. Re-scored:

| Dimension | Score | Evidence |
|---|---|---|
| Correctness & coverage | 4→5 | 115 tests green (inflate vectors, stored-zip round-trip, real-jar fixtures) |
| Decompilation fidelity | 5 | unchanged outputs, now reachable from jars |
| Maintainability | 4 | 0 clippy warnings |
| Docs alignment | 5 | docs_sync enforced |
| Wiring & ergonomics | 4→5 | same CLI covers .dll/.exe/.class/.jar; recursive mode includes jars |
| Footprint | 5 | still one dependency (`clap`) |

**28/30.** Remaining: body-level generic inference and lambda reconstruction
(both usage-based inference problems), Zip64 archives.
