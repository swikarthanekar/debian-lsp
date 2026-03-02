# Semantic Layer Prototype for debian-lsp

## Motivation

While reviewing the current architecture of `debian-lsp`, most functionality appeared to operate in a request-local manner:

parse → respond

There is no persistent semantic model representing relationships between packages across the workspace.

Given the broader goal of integrating with `debian-analyzer` and enabling richer IDE features such as hover, go-to-definition, and cross-file navigation, this prototype explores introducing a lightweight semantic layer as a proof of concept.

---

## What This Prototype Adds

### 1. Separate Semantic Workspace Layer
- Fully isolated from the existing workspace
- Non-intrusive to current parsing and request handling logic

### 2. Persistent Symbol Index
- Indexes `Package:` definitions across the workspace
- Maintains a bidirectional dependency graph

### 3. Real-World Dependency Parsing Support
Handles:

- Version constraints (e.g., libfoo (>= 1.2))
- Architecture qualifiers (e.g., python3:any)
- Comma-separated dependency lists
- Multiline continuation fields

### 4. Cross-File Go-To-Definition
- Navigate from dependency usage to its `Package:` definition
- Works across multiple files

### 5. Reverse Dependency Graph
- Tracks dependents for each package
- Enables semantic graph-style queries.

### 6. Hover Integration
- Displays reverse dependency relationships
- Provides contextual semantic information
- Hover over a package shows:
    - Package name
    - Reverse dependency list (Used by: …)

### 7. Test Coverage
- Unit tests validating:
  - Dependency parsing
  - Graph construction
- No regression in existing test suite

---

## Design Choices

### Isolation First
The semantic layer is intentionally separated from existing parsing logic to:

- Avoid modifying core workspace behavior
- Keep the prototype low-risk
- Preserve current functionality

### Bidirectional Graph Index

The index is structured as a graph to enable future features:

- Find references
- Dependency cycle detection
- Unused package diagnostics

### Structured API

The semantic layer exposes a structured interface to allow:

- Future integration with `debian-analyzer`
- Gradual replacement of text-based parsing
- Clear separation of semantic modeling from syntax parsing

---

## Future Direction

The current implementation performs lightweight text-based indexing.

A natural next step would be:

- Integrating with the existing deb822 AST
- Eliminating duplicate parsing
- Enabling field-aware semantic analysis

This would allow the semantic model to operate directly on structured representations instead of raw text parsing.

---

## Status

This is a proof-of-concept prototype designed to validate architectural direction while remaining non-invasive to the current system.