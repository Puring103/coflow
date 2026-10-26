# Localization and dimensions

`@localized` binds a field to the `language` dimension; `@dimension("name")` binds it to a named dimension. A field stores a base value and any number of variant values.

```cft
table Item {
  @localized
  name: string;
  @dimension("region")
  price: int;
}
```

```cfd
sword: Item { name: "Sword", price: 100 }
language[ja] { sword: Item { name: "剣" } }
region[cn] { sword: Item { price: 90 } }
```

Override blocks target existing records and replace only explicitly supplied dimension fields. The field must be bound to the enclosing dimension and the value must match its declared type. Duplicate overrides for the same dimension, variant, record and field are errors.

Variants are collected from override blocks in all selected CFD files. File order does not establish priority. Missing overrides and unknown variants use the base value. Explicit `None` on an Option field is a valid override and preserves that layer's empty state.

In checks, direct field access returns the base value; `.for("ja")` returns the effective variant value; `.hasVariant("ja")` checks for an explicit override. `.variants()` returns effective values for every discovered variant in the field's dimension, including variants without an override on that field.

```cft
check TranslationComplete {
  all variant, text in name.variants() {
    name.hasVariant(variant): f"Missing {variant} name";
    text.len() > 0: f"The {variant} name cannot be empty";
  }
}
```

Place this rule inside a type with a `name` field. To require a variant that has not appeared anywhere in the project, explicitly check `name.hasVariant("ja")`.

In C#, `item.name` returns the base string. Use `item.For("language", "ja").name` as needed; different consumers can hold different views simultaneously. `item.HasVariant("name", "ja")` checks explicit coverage and `item.Variants("language")` lists known variants. Chain `For` to combine dimensions, for example `item.For("language", "ja").For("region", "cn")`.

Templates read dependencies in the current dimension context. If a language field `title` has the base expression `f"Buy {self.name}"` and Japanese overrides only `name` with `"剣"`, `title.for("ja")` returns `"Buy 剣"` while `title.hasVariant("ja")` remains `false`. Fallback evaluates the base expression in the selected context. A same-named variant in another dimension is not selected automatically.

## Query explicit coverage

The CLI queries coverage and effective values separately:

```powershell
coflow dimension get --type Item --id sword --field name --dimension language --variant ja --json
coflow dimension missing --dimension language --variant ja --json
```

`dimension get` returns `explicit` for field coverage, `override` for the supplied override and `effective` for the resulting value. `variant_exists` reports whether the variant exists anywhere in the dimension. Explicit `None` and values equal to the base value both count as supplied overrides.

`dimension missing` lists fields without explicit overrides, useful for translation completeness. `dimension clear` removes an override and restores base-value reading; the field then counts as missing again.

## Files and editing

Configure editor override directories in `coflow.yaml`:

```yaml
dimensions:
  language:
    display_name: Localization
    dir: dimensions/language/
  region:
    display_name: Region
    dir: dimensions/region/
```

Directories recursively include CFD without duplicate `data` entries. Missing directories are treated as empty. When an override is first filled in, the editor creates its file and preserves the base file's full project-relative path. For example, language overrides for `data/equipment/items.cfd` go to `dimensions/language/data/equipment/items.cfd`. All records, fields and variants of the same base file are grouped per dimension, retaining the filename `items.cfd`.

This is the editor's recommended organization. Override blocks may also live in any selected CFD input, including beside base records. Existing overrides are saved to their original files. Renaming or deleting a base record updates its override targets.

The file tree separates schema, base data and dimensions. Dimension trees expose actual directories and CFD files. Open a file to edit its override fields in table or record views. Overrides from other sources show and retain their actual source. Pure override files appear in the dimension tree; files also containing base records remain in the data tree.

Right-click a base file inside the project and choose localization editing or the relevant dimension to open its recommended file. Directories and files are created on the first filled value. Add a variant to create a pending column, then fill and save overrides. Configure the dimension's `dir` before adding overrides. Overrides for base files outside the project can be edited in existing override files.
