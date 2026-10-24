## Design principles

Apply these deliberately. They are not decoration:

- **Single responsibility.** One reason to change per module, type and component. Split a file when it starts doing
  several jobs; keep request/response DTOs apart from the logic that produces them.
- **Open/closed.** Extend by adding, not editing: a new kind of thing is a new implementation of an existing trait or a
  new configuration entry, not a special case in shared code.
- **Liskov.** Every implementation of a trait or interface honours its documented contract: same error semantics, no
  surprising panics.
- **Interface segregation.** Keep traits and TypeScript interfaces small and purpose-built.
- **Dependency inversion / code against abstractions.** Tauri commands depend on traits (ports), never on a concrete
  store, pipe or OS API. Angular components never call Tauri `invoke` directly: they go through the backend injected by
  DI token (the real Tauri backend, or the mock for browser development and tests), and state lives in SignalStores.
- **Pure core, thin edges.** Parsing, rules, formatting, geometry and state machines are pure functions with time and
  I/O passed in; processes, files, network and the OS stay thin.
- **Ports and adapters.** The core never depends on an adapter (Tauri, a database, an OS API).
- **Parse, don't validate.** Convert untrusted input (file bytes, IPC JSON, config files, network replies) into typed
  values once, at the edge, and work with the types afterwards.
- **Make illegal states unrepresentable.** Prefer enums and newtypes over strings, flags and `Option` combinations that
  can disagree.
- **Command-query separation.** Queries have no side effects; only explicit commands mutate.
- Also: separation of concerns, high cohesion and low coupling, KISS, YAGNI, DRY with the rule of three before
  abstracting, composition over inheritance, Law of Demeter, least astonishment (sensible defaults, per-item overrides),
  fail fast with a message that tells the user what to fix, and leave touched code better than you found it.
