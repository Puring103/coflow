# Error code index

Codes identify diagnostic categories. Source positions, record coordinates and field paths are supplied by the diagnostic structure.

Use `coflow explain <code> --json` for meaning, triggers, repairs and suggested queries. `check --explain` and `schema check --explain` explain current diagnostics.

```bash
coflow explain CFT-SCHEMA-006 --json
coflow explain REF-001 --json
```

## CFT declarations and check expressions

| Code | Meaning | Trigger | Repair |
| --- | --- | --- | --- |
| `CFT-LEX-001` | Invalid character | Source contains an unsupported character | Remove it or use a valid identifier or symbol. |
| `CFT-LEX-002` | Invalid string escape | A string escape sequence is invalid | Rewrite it using supported escapes. |
| `CFT-LEX-003` | Unterminated string | Closing quote is missing | Add the quote and check escaping. |
| `CFT-LEX-004` | Invalid integer literal | Integer format or range is invalid | Use a valid signed 64-bit integer literal. |
| `CFT-LEX-005` | Invalid floating-point literal | Floating-point format or range is invalid | Use a valid finite floating-point literal. |
| `CFT-SYN-001` | Unexpected token | Token is invalid at this position | Correct the symbol or syntax at the diagnostic position. |
| `CFT-SYN-002` | Unexpected end of source | File ends before a construct is complete | Add the missing value, separator or closing delimiter. |
| `CFT-SYN-003` | Expected identifier | A declaration or path requires a name | Supply a valid identifier. |
| `CFT-SYN-004` | Missing symbol | Required syntax symbol is absent | Add the symbol specified by the message. |
| `CFT-SYN-005` | Invalid top-level declaration | Unsupported top-level declaration form | Use a supported CFT declaration. |
| `CFT-SYN-006` | Invalid comparison chain | Unsupported chained comparisons | Split comparisons and join them with boolean operators. |
| `CFT-SYN-007` | Invalid check placement | Fields appear after a type's check blocks | Put fields before checks. |
| `CFT-SYN-008` | Invalid annotation syntax | Annotation name or argument structure is invalid | Correct the name and arguments. |
| `CFT-SYN-009` | Invalid statement or empty rule | Check is empty or contains invalid statements | Add valid assertions and correct the indicated statement. |
| `CFT-SYN-010` | Duplicate rule in inheritance chain | Current type or ancestors contain a same-named check | Give conflicting rules unique names, preserving declaration order. |
| `CFT-SYN-011` | Syntax structure limit exceeded | Nesting or source structure exceeds limits | Reduce nesting or split the structure. |
| `CFT-SCHEMA-001` | Duplicate module | Schema inputs yield duplicate module identities | Adjust input scope to remove duplicates. |
| `CFT-SCHEMA-002` | Duplicate global name | Multiple declarations share a name | Give declarations different names. |
| `CFT-SCHEMA-003` | Duplicate field name | A type declares a field more than once | Merge or rename duplicate fields. |
| `CFT-SCHEMA-004` | Duplicate enum member | An enum repeats a member name | Remove or rename duplicate members. |
| `CFT-SCHEMA-005` | Duplicate enum value | Enum members share a numeric value | Assign distinct valid values. |
| `CFT-SCHEMA-006` | Undeclared type | A type name cannot be resolved | Declare it or use an existing type. |
| `CFT-SCHEMA-007` | Base is not a type | Base name refers to an ineligible declaration | Select an inheritable type declaration. |
| `CFT-SCHEMA-008` | Undeclared constant | Constant name cannot be resolved | Declare it or use an existing constant. |
| `CFT-SCHEMA-009` | Inheritance cycle | Base-type relationships form a cycle | Remove the cyclic inheritance edge. |
| `CFT-SCHEMA-010` | Inheriting sealed type | Base type forbids inheritance | Select an inheritable base or adjust the declaration. |
| `CFT-SCHEMA-011` | Inherited field conflict | Local field conflicts with an inherited name | Keep one declaration or use distinct names. |
| `CFT-SCHEMA-012` | Conflicting modifiers | Type modifiers cannot coexist | Use a valid modifier combination. |
| `CFT-SCHEMA-014` | Invalid dictionary key type | Unsupported key type | Use an allowed dictionary key type. |
| `CFT-SCHEMA-015` | Invalid default expression | Default cannot be parsed as a default value | Rewrite it for the field type. |
| `CFT-SCHEMA-016` | Default type mismatch | Default is not assignable to the field | Adjust the value or declared type. |
| `CFT-SCHEMA-017` | Invalid default field reference | Default depends on a prohibited field reference | Use constants or a valid default expression. |
| `CFT-SCHEMA-018` | Invalid enum value sequence | Explicit or automatic sequence is invalid | Adjust values and declaration order. |
| `CFT-SCHEMA-019` | Invalid flag value | Member violates flag rules | Use values allowed by flag enum rules. |
| `CFT-SCHEMA-020` | Unknown annotation | Unsupported annotation name | Use a supported annotation. |
| `CFT-SCHEMA-021` | Duplicate annotation | A nonrepeatable annotation is repeated | Keep one instance. |
| `CFT-SCHEMA-022` | Annotation without target | No following declaration to bind | Place it before a supported declaration. |
| `CFT-SCHEMA-023` | Invalid annotation target | Declaration category is unsupported | Apply it to an allowed target. |
| `CFT-SCHEMA-024` | Invalid annotation arguments | Wrong argument count or values | Follow the annotation's argument requirements. |
| `CFT-SCHEMA-025` | Invalid annotated field type | Field violates annotation constraints | Adjust the field type or annotation. |
| `CFT-SCHEMA-026` | Struct requires sealed | Struct declaration is not sealed | Add sealed as required by struct rules. |
| `CFT-SCHEMA-027` | Nonempty idAsEnum target | Target enum already declares members | Use an empty enum satisfying idAsEnum rules. |
| `CFT-SCHEMA-028` | Member target is not an enum | Enum member access targets another declaration | Use the correct enum type. |
| `CFT-SCHEMA-029` | Unknown enum member | Named member is absent | Select a declared member. |
| `CFT-SCHEMA-030` | Invalid constant value | Expression cannot produce a valid constant | Rewrite according to constant type and expression rules. |
| `CFT-SCHEMA-031` | Reserved identifier | Declaration uses a reserved name | Use a valid nonreserved identifier. |
| `CFT-SCHEMA-034` | Invalid localized target | Localization annotation targets unsupported declaration | Follow localization target rules. |
| `CFT-SCHEMA-035` | Invalid localization name | Name is not a valid identifier | Use a valid localization name. |
| `CFT-SCHEMA-032` | Invalid dimension target | Dimension annotation targets unsupported declaration | Follow dimension target rules. |
| `CFT-SCHEMA-033` | Invalid dimension name | Name is not a valid identifier | Use a valid dimension name. |
| `CFT-SCHEMA-036` | Abstract singleton | Singleton is combined with abstract | Use a concrete singleton-compatible declaration. |
| `CFT-SCHEMA-037` | Singleton/idAsEnum conflict | Incompatible identity annotations are combined | Adjust annotations to match identity rules. |
| `CFT-SCHEMA-038` | Schema structure limit exceeded | Declaration size or dependencies exceed limits | Reduce size or dependency nesting. |
| `CFT-SCHEMA-039` | Duplicate project rule | Top-level checks share a name | Give project rules unique names. |
| `CFT-SCHEMA-040` | Alias cycle | Alias expansion is cyclic | Remove cyclic dependencies. |
| `CFT-SCHEMA-042` | Required object cycle | Required object fields cannot terminate | Make the object structure finite. |
| `CFT-SCHEMA-043` | Default expansion cycle | Default objects expand cyclically | Remove cyclic default dependencies. |
| `CFT-TYPE-001` | Unknown value name | Check cannot resolve a value name | Use a valid field, constant or binding. |
| `CFT-TYPE-002` | Unknown field | Check accesses a nonexistent field | Select a valid field on the target type. |
| `CFT-TYPE-003` | Unknown enum member in check | Check references an undeclared member | Select a valid member. |
| `CFT-TYPE-004` | Invalid enum target in check | Member access targets a non-enum | Use the correct enum type. |
| `CFT-TYPE-005` | Operator type mismatch | Operand types do not support the operator | Adjust operands or operator. |
| `CFT-TYPE-006` | Comparison type mismatch | Values cannot be compared this way | Use matching, comparable types. |
| `CFT-TYPE-007` | Nonboolean condition | Condition does not produce bool | Use a boolean expression. |
| `CFT-TYPE-008` | Unknown check function | Unsupported function call | Use a supported check function. |
| `CFT-TYPE-009` | Wrong argument count | Call does not match function signature | Add or remove arguments. |
| `CFT-TYPE-010` | Wrong argument type | Argument types do not match signature | Adjust argument values or types. |
| `CFT-TYPE-011` | Field access on non-object | Current path value is not an object | Correct the path for the declared type. |
| `CFT-TYPE-012` | Nonindexable target | Indexing is unsupported for the target | Use an indexable collection. |
| `CFT-TYPE-013` | Index type mismatch | Index or key has the wrong type | Match the collection's index or key type. |
| `CFT-TYPE-014` | Invalid type test | Invalid target or type in is expression | Follow type-test rules. |
| `CFT-TYPE-015` | Quantifier target is not a collection | Iteration target is unsupported | Use a valid collection or record set. |
| `CFT-TYPE-016` | Invalid unique element type | Element type is unsupported for unique | Use supported elements or projection. |
| `CFT-TYPE-017` | Invalid bitwise types | Operands are neither integers nor compatible flags | Use int or compatible flag values. |
| `CFT-TYPE-018` | Shift requires integers | Shift operand is not int | Use integer operands. |
| `CFT-TYPE-019` | Regex pattern is not a literal | Pattern is not the required literal form | Use a string pattern literal. |
| `CFT-TYPE-020` | Invalid regex pattern | Pattern cannot compile | Correct regex syntax. |
| `CFT-TYPE-021` | Invalid quantifier bindings | Binding number or form mismatches the collection | Follow collection or record-set binding rules. |
| `CFT-TYPE-022` | Invalid record collection query | Query target or form is invalid | Use a valid record type and query form. |

## Data model, references and rule execution

| Code | Meaning | Trigger | Repair |
| --- | --- | --- | --- |
| `DATA-001` | Unknown record type | Record type cannot be resolved | Declare it or use an existing record type. |
| `DATA-002` | Abstract record type | Record directly uses an abstract type | Use an assignable concrete type. |
| `DATA-003` | Missing object type | Object type cannot be determined | Supply a valid, determinable object type. |
| `DATA-004` | Object type mismatch | Actual type is not assignable | Use a concrete object assignable to the field. |
| `DATA-005` | Undeclared data field | Record or object contains an extra field | Remove it or add the field declaration. |
| `DATA-006` | Missing required field | No assignment or usable default | Supply a valid field value. |
| `DATA-007` | Data value type mismatch | Value is not assignable to the field | Supply the declared type. |
| `DATA-008` | Invalid enum value | Value violates target enum constraints | Use a valid member or allowed flag combination. |
| `DATA-009` | Duplicate dictionary key | Keys are semantically equal | Merge or remove duplicate entries. |
| `DATA-010` | Missing record identity | Required identity information is absent | Complete identity according to record rules. |
| `DATA-011` | Duplicate record key | Repeated key in one identity scope | Use unique record keys. |
| `DATA-012` | Duplicate polymorphic record key | Keys conflict in a polymorphic identity scope | Adjust conflicting keys. |
| `DATA-013` | Invalid record key | Key violates identity rules | Use a valid key for the target type. |
| `DATA-014` | Value dependency cycle | Values or interpolation form a cycle | Remove the cyclic dependency path. |
| `DATA-015` | Invalid singleton count | Record count violates singleton requirements | Correct the singleton record count. |
| `DATA-016` | Invalid singleton key | Record does not use its required identity | Use the prescribed singleton key. |
| `DATA-017` | Singleton key conflict | Singleton identity conflicts with another record | Remove the identity conflict. |
| `DATA-018` | Data structure limit exceeded | Size or nesting exceeds limits | Reduce structure size or depth. |
| `REF-001` | Missing reference target | Target record cannot be found | Create the target or correct the reference key. |
| `REF-002` | Reference target type mismatch | Target is not assignable to the reference type | Choose a compatible target record. |
| `CHECK-001` | Business rule failed | Business condition is false | Inspect the rule and located values. |
| `CHECK-002` | Evaluation type error | Runtime value type violates operation requirements | Inspect fields, expressions and dependencies. |
| `CHECK-004` | Array index out of bounds | Rule reads an absent array position | Correct the index or guard collection bounds. |
| `CHECK-005` | Missing dictionary key | Rule reads an absent key | Add the key or check membership first. |
| `CHECK-006` | Extremum of empty collection | min or max receives an empty collection | Supply values or check nonemptiness first. |
| `CHECK-007` | Comparison failed | Comparison evaluates to false | Inspect both values and their intended relationship. |
| `CHECK-008` | Boolean condition failed | Required true condition is false | Inspect dependencies and business requirements. |
| `CHECK-009` | Negated condition failed | Negation does not satisfy the rule | Inspect the negated condition's values. |
| `CHECK-010` | Conjunction failed | Not all conditions hold | Inspect failed subconditions and related values. |
| `CHECK-011` | Disjunction failed | No condition satisfies requirements | Inspect alternatives and related values. |
| `CHECK-012` | Type test failed | Actual type does not satisfy the test | Inspect actual type or type condition. |
| `CHECK-014` | Contains check failed | Collection or string lacks the required value | Inspect contents, target and membership condition. |
| `CHECK-015` | Uniqueness check failed | Values required to be unique repeat | Correct duplicates or uniqueness projection. |
| `CHECK-016` | Pattern match failed | String does not match required regex | Adjust text or business pattern. |
| `CHECK-017` | Any check failed | No element satisfies the condition | Inspect collection and quantifier condition. |
| `CHECK-018` | None check failed | An element satisfies the prohibited condition | Inspect matching elements and prohibition. |
| `CHECK-019` | All check failed | Some element does not satisfy the condition | Inspect failed elements and condition. |
| `CHECK-020` | Execution budget exhausted | Rule work reaches the budget | Reduce work or adjust the plan; recheck completion. |

## Projects, files and command execution

| Code | Meaning | Trigger | Repair |
| --- | --- | --- | --- |
| `PROJECT-CONFIG-NOT-FOUND` | Configuration not found | No project config in the selected directory or parents | Specify project directory or file with --project. |
| `PROJECT-CONFIG-AMBIGUOUS` | Ambiguous configuration | Multiple supported config filenames coexist | Keep the intended file or explicitly select its path. |
| `PROJECT-CONFIG-PATH` | Invalid configuration path | Path is absent, wrong kind or unresolvable | Check --project and file access. |
| `PROJECT-CONFIG-READ` | Configuration read failed | Config cannot be read | Check path, permissions and file state. |
| `PROJECT-CONFIG-PARSE` | Configuration parse failed | coflow.yaml violates syntax or field types | Correct the indicated configuration content. |
| `PROJECT-CONFIG-WRITE` | Configuration write failed | Config update could not be written | Check target path and write access. |
| `PROJECT-CONFIG-OVERLAP` | Overlapping inputs | Schema and data cover the same source | Separate schema and data input scopes. |
| `PROJECT-SCHEMA-PATH` | Invalid schema path | Input is missing or malformed | Check schema paths and file kinds. |
| `PROJECT-SCHEMA-READ` | Schema read failed | File or directory cannot be read | Check inputs and access. |
| `PROJECT-CWD` | Current directory unavailable | Discovery cannot obtain calling directory | Run from an accessible directory or specify the project. |
| `PROJECT-INIT-IO` | Initialization failed | I/O error creating project files | Check target directory and creation permissions. |
| `PROJECT-FILE-CREATE` | File creation failed | Project file cannot be created | Check existing targets and directory write access. |
| `PROJECT-FILE-DELETE` | File deletion failed | Project file cannot be deleted | Check file and deletion permissions. |
| `PROJECT-FILE-RESTORE` | File restoration failed | Recovery after a failed operation is incomplete | Verify actual contents at the diagnostic paths and restore. |
| `CFD-READ` | CFD write source read failed | Required file or source structure cannot be read | Check source file, record location and diagnostic. |
| `CFD-WRITE` | CFD source edit failed | Request cannot safely apply to source structure | Check path, record source and requested operation. |
| `CFD-SOURCE` | Invalid CFD input source | Input is not a valid CFD source | Check data input configuration and file kinds. |
| `CLI-REQUEST` | Invalid request | Arguments or structured input violate command protocol | Query describe and correct the request. |
| `CLI-TARGET` | Target not found | Requested type, record, field or other target is absent | Query identity again and select an existing target. |
| `CLI-CONFLICT` | Input or precondition conflict | Input changed or expected condition is unmet | Reread current content and prepare a new request. |
| `CLI-IO` | Request I/O failed | Request file, stdin or other file operation failed | Check diagnostic paths and access. |
| `CLI-PUBLISH` | Candidate publication failed | Publication or recovery did not finish | Verify written_files, restored_files and restoration_failed before preparing another request. |
| `CLI-CANDIDATE` | Candidate processing failed | Candidate construction, path mapping or source identification failed | Check input paths, candidate config and diagnostics. |
| `CLI-FIELD` | Invalid field projection | Path cannot resolve over selected types | Query schema and select a valid field. |
| `CLI-PATH` | Invalid value path | Field or collection path mismatches target structure | Correct field name, index or dictionary key. |
| `CLI-SORT` | Invalid sort field | Field cannot sort the selected scope | Use valid, consistently typed sortable fields. |
| `CLI-FILTER` | Invalid filter | Filter fields or value types are invalid | Follow --where syntax and declared types. |
| `CLI-VALUE` | Invalid write value | Expression or model value mismatches target type | Supply a valid value for the field. |
| `ARTIFACT-001` | Code artifact processing failed | Output directory, stable map or publication is invalid | Check targets, input scopes and publication/recovery results. |
| `ARTIFACT-IO` | Artifact I/O failed | Output or staging file cannot be read, created or written | Check paths and permissions; use publication/recovery results before retrying. |
| `CFD-TEXT-Syntax` | Invalid CFD syntax | Source cannot be fully parsed | Correct syntax at the indicated position and query again. |
| `CHECK-BUDGET` | Rule planning budget exhausted | Rule task count exceeds planning budget | Narrow task scope or adjust rules and recheck. |
| `CLI-LSP` | Language service startup failed | Server execution or transport failed | Check project path, I/O channels and diagnostics. |
| `DIM-CONFIG-002` | Invalid dimension directory | Directory conflicts with inputs or other dimensions | Correct dimension directory configuration. |
| `GIT-DIFF` | Git diff read failed | Repository, baseline or historical file cannot be read | Check repository and baseline; use diff --input for candidates. |
| `GIT-EXTERNAL-SOURCE` | Historical external input unavailable | Historical config references inputs outside the repository | Verify listed historical paths and incomplete scope. |
| `PROJECT-001` | Invalid config or generation target | Input, target or stable enum map violates requirements | Correct located input and target configuration. |
| `RUNTIME-INTERNAL` | Invalid internal runtime state | Project processing encounters an inconsistency | Preserve diagnostics and inputs for investigation. |
| `SCHEMA-STDIN-PATH` | Invalid stdin schema path | Path cannot map to a valid module | Use a valid project-relative CFT path. |
| `MUTATION-COORDINATE` | Invalid record coordinate | Type or key violates identity rules | Use valid identity returned by queries. |
| `MUTATION-DEFAULT` | Default materialization failed | Default cannot become an editable value | Check defaults and nested type declarations. |
| `MUTATION-DIMENSION` | Invalid dimension target | Record, binding or variant violates requirements | Verify identity, dimension binding and variant name. |
| `MUTATION-DIMENSION-DESTINATION` | Invalid dimension destination | Directory policy cannot determine a valid new override file | Configure dimension directories; explicitly select a destination for external base records. |
| `MUTATION-DIMENSION-PATH` | Invalid dimension value path | Nested path mismatches field type | Match the field declaration and actual object type. |
| `MUTATION-DIMENSION-STALE` | Stale dimension condition | Explicit override differs from expected state | Reread the override and prepare a new request. |
| `MUTATION-FILE` | Invalid mutation file | Target is outside permitted project sources | Use a selected CFD source file. |
| `MUTATION-FILE-GUARD` | Source file condition conflict | Fingerprint differs from the request | Reread source and update preconditions. |
| `MUTATION-INSERT` | Invalid record creation | Type, key or source is invalid | Use a concrete record type and valid source. |
| `MUTATION-INSERT-CONFLICT` | Record creation identity conflict | New identity conflicts with candidate records | Query identities and choose a unique key. |
| `MUTATION-PATH` | Invalid mutation path | Path cannot locate the value | Verify fields, indices and typed keys. |
| `MUTATION-RENAME` | Invalid rename request | Target or new identity is invalid | Use a valid record identity and new key. |
| `MUTATION-RENAME-CONFLICT` | Rename identity conflict | New key conflicts with records or batch requests | Choose a nonconflicting key. |
| `MUTATION-REORDER` | Invalid reorder request | Target or position is invalid | Verify identity and target position. |
| `MUTATION-REORDER-BATCH` | Invalid batch reorder | Batch contains incompatible reorder operations | Organize operations by source container. |
| `MUTATION-REORDER-CONTAINER` | Reorder containers differ | Records do not share a writable source container | Reorder within the same CFD file. |
| `MUTATION-SHAPE` | Invalid mutation shape | Value structure mismatches field shape | Supply objects, collections or primitives matching the declaration. |
| `MUTATION-TRANSFER` | Invalid transfer request | Record or destination is invalid | Verify source record and target file. |
| `MUTATION-TRANSFER-FILE` | Invalid transfer file | Destination is not a valid CFD file | Select a permitted CFD target path. |
| `MUTATION-TXN-INVARIANT` | Inconsistent transaction state | Candidate plan or source index is inconsistent | Preserve diagnostics and request for investigation. |
| `MUTATION-TYPE` | Invalid mutation type | Target or actual value type is unsuitable | Verify field declaration and actual object type. |
| `MUTATION-VALUE` | Invalid mutation value | Submitted value violates type or reference constraints | Correct the value using the field type and diagnostics. |
| `WRITE-AMBIGUOUS-SOURCE` | Ambiguous write source | Multiple sources map to the same target file or record | Resolve ambiguity before retrying. |
| `WRITE-NO-SOURCE` | Missing write source | Target has no locatable source | Use an existing project source or create one. |
| `WRITE-NOT-FOUND` | Write record not found | Source session lacks the target | Query identity again before submitting. |
| `WRITE-RENAME` | Invalid record key value | Rename lacks a valid string | Supply a valid new key. |
| `WRITE-REORDER-CONTAINER` | Reorder sources differ | Records do not share a writable CFD file | Reorder within one source file. |
| `WRITE-REORDER-INDEX` | Reorder position out of bounds | Target exceeds source container length | Use a valid container position. |
| `WRITE-REORDER-ORIGIN` | Unwritable reorder source | Record has no writable origin | Use CFD-sourced records. |
| `WRITE-REORDER-TYPE` | Unsupported reorder source | Source writer does not support the operation | Verify origin and supported operations. |
| `WRITE-SHAPE` | Invalid write structure | Source field or value cannot be safely edited | Verify source structure and request. |
| `WRITE-TRANSFER-FILE` | Invalid transfer file | Source or destination violates requirements | Use valid CFD files and verify source scope. |
| `WRITE-TRANSFER-INDEX` | Transfer position out of bounds | Insertion exceeds target record count | Use a valid insertion position. |
| `WRITE-TRANSFER-ORIGIN` | Unwritable transfer source | Record lacks writable source text | Use records with CFD source origins. |
| `SELF-UPDATE-CONFIG` | Invalid update configuration | Service parameters are missing or unreadable | Check service config and version information. |
| `SELF-UPDATE-CONFIRM` | Update confirmation read failed | Input channel cannot read confirmation | Use an interactive terminal or explicit --yes. |
| `SELF-UPDATE-INSTALL` | CLI update install failed | Download, replacement or installation failed | Check network, install path and permissions. |
| `SELF-UPDATE-OUTPUT` | Update prompt output failed | Terminal cannot display confirmation | Check terminal I/O channels. |
| `SELF-UPDATE-PLATFORM` | Unsupported update platform | No artifact for this OS or architecture | Use a release artifact for a supported platform. |
| `SELF-UPDATE-RELEASE` | Release lookup failed | Release data unavailable or artifact missing | Check network and release service response. |
| `SELF-UPDATE-VERSION` | Invalid update version | Installed or remote version cannot be parsed | Check installation and release identifiers. |
| `SKILL-HOME` | Skill home unavailable | Global install cannot locate a user directory | Configure a valid user or agent config directory. |
| `SKILL-DIR-CREATE` | Skill directory creation failed | Target directory cannot be created | Check path and write permissions. |
| `SKILL-REMOVE` | Skill removal failed | Installed files cannot be removed | Check permissions and open-file locks. |
| `SKILL-BUNDLE` | Invalid skill bundle | Distribution has missing files or invalid structure | Check CLI distribution integrity. |
| `SKILL-SWAP` | Skill replacement failed | Prepared bundle cannot replace target | Check installation permissions and file state. |
| `SKILL-CLEANUP` | Skill temporary cleanup failed | Temporary directory or backup cannot be removed | Clean remaining files at diagnostic paths. |
| `SKILL-FILE-WRITE` | Skill file write failed | Files cannot be written to target | Check directory access and disk state. |
| `SKILL-MANIFEST` | Invalid install manifest | Manifest unreadable, malformed or incompatible | Check path and installation integrity. |
| `SKILL-DIR-READ` | Skill directory read failed | Directory cannot be enumerated or read | Check directory access. |
| `SKILL-DIR-REMOVE` | Skill directory deletion failed | Uninstall target cannot be deleted | Check permissions and locks. |
| `CFD-TEXT-UnknownType` | Unknown CFD record or object type | Type marker is absent from schema | Use a declared concrete type. |
| `CFD-TEXT-AbstractObjectType` | Abstract CFD object type | Object actual type cannot be constructed | Use an assignable concrete object type. |
| `CFD-TEXT-ObjectTypeMismatch` | CFD object type mismatch | Type marker is not assignable to the field | Select a compatible actual type. |
| `CFD-TEXT-UnknownField` | Undeclared CFD field | Assignment names a nonexistent field | Query declarations and correct the name. |
| `CFD-TEXT-DuplicateField` | Duplicate CFD assignment | Same object or record assigns a field repeatedly | Merge or remove duplicates. |
| `CFD-TEXT-ReservedIdField` | Reserved identity field assigned | Identity is treated as an ordinary field | Use the record key or rename operation. |
| `CFD-TEXT-TypeMismatch` | CFD expression type mismatch | Expression cannot load as the declared field type | Correct expression for the field type. |
| `CFD-TEXT-InvalidEnumVariant` | Invalid CFD enum expression | Member or bit combination violates the target enum | Use valid members or allowed flag combinations. |
| `CFD-TEXT-ReferenceNeedsMarker` | Missing reference marker | Reference omits its marker | Use &key or &Type::key. |
| `WRITE-CONFLICT` | Write source content conflict | External changes occurred after preparation | Reread source and prepare the edit again. |
