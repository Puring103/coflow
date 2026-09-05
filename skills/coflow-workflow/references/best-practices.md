# Best practices

Keep schema, CFD data and generated source directories separate. Run `coflow check` after every schema or CFD edit, then run `coflow codegen` when generated bindings are needed. Keep generated directories out of hand edits and pin runtime package versions with the generated target.
