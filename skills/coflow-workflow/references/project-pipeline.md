# Project pipeline

`cft check` compiles schema. `check` loads CFD and evaluates checks. `codegen` renders configured target-language source files. `build` runs check and all codegen targets with atomic publication. Failed generations never replace a successful generation.
