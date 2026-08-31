# Keep the core language-neutral and implemented in Rust

Rust owns Gloom's domain semantics, evidence validation, identity, indexing,
queries, orchestration, and local interfaces. Language- and toolchain-specific
coupling stays behind versioned evidence-contributor contracts, allowing LLVM
and future language-aware components to evolve without defining the core model.
An out-of-process C++ LLVM contributor is a leading hypothesis rather than part
of this decision; its boundary must be validated before commitment.
