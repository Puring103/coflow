use coflow_format::{format_cfd, format_cft};

#[test]
fn formatter_ignores_delimiters_inside_strings_and_comments() {
    let source = "type Item {\n\
values: [string] = [\n\
\"{\" # string brace does not indent\n\
] # closing bracket in comment } }\n\
}\n";

    assert_eq!(
            format_cft(source),
            "type Item {\n  values: [string] = [\n    \"{\" # string brace does not indent\n  ] # closing bracket in comment } }\n}\n"
        );
    assert_eq!(
        format_cft("type Item {\n\nkey: string;\n}"),
        "type Item {\n  key: string;\n}\n"
    );
    assert_eq!(
        format_cft(
            "check ItemRules {\nall item in records(Item) {\nitem.value > 0:\n \"bad {item.id}\";\n}\n}"
        ),
        "check ItemRules {\n  all item in records(Item) {\n    item.value > 0:\n      \"bad {item.id}\";\n  }\n}\n"
    );
}
#[test]
fn formatter_covers_all_new_default_forms_and_is_idempotent() {
    let source = "type Rule {\n\
label:string=\"rule {name} {{literal}}\";\n\
mask:int=READ|WRITE;\n\
effect:Effect=HealEffect { amount:1,label:\"heal {amount}\",};\n\
apply:fn(value:int)->int=fn(input:int)->int {\n\
var total=input+1;\n\
total\n\
};\n\
}\n";
    let expected = concat!(
        "type Rule {\n",
        "  label: string = \"rule {name} {{literal}}\";\n",
        "  mask: int = READ | WRITE;\n",
        "  effect: Effect = HealEffect { amount: 1, label: \"heal {amount}\", };\n",
        "  apply: fn(value: int) -> int = fn(input: int) -> int {\n",
        "    var total = input + 1;\n",
        "    total\n",
        "  };\n",
        "}\n",
    );
    assert_eq!(format_cft(source), expected);
    assert_eq!(format_cft(expected), expected);
}

#[test]
fn formatter_preserves_depth_across_close_then_open_lines() {
    let functions = "default_calculator: Calculator {\n\
add: fn(left: int, right: int) -> int {\n\
left + right\n\
},\n\
classify: fn(value: int) -> string {\n\
if value >= 10 {\n\
\"large\"\n\
} else {\n\
\"small\"\n\
}\n\
},\n\
}\n";
    assert_eq!(
        format_cft(functions),
        "default_calculator: Calculator {\n  add: fn(left: int, right: int) -> int {\n    left + right\n  },\n  classify: fn(value: int) -> string {\n    if value >= 10 {\n      \"large\"\n    } else {\n      \"small\"\n    }\n  },\n}\n"
    );

    let polymorphic_array = "starter_effects: EffectBundle {\n\
primary: HealEffect {\n\
amount: 0,\n\
label: \"\",\n\
},\n\
additional: [HealEffect {\n\
amount: 5,\n\
label: \"Recovery\",\n\
}, HealEffect {\n\
amount: 5,\n\
label: \"Recovery\",\n\
}],\n\
}\n";
    assert_eq!(
        format_cfd(polymorphic_array),
        "starter_effects: EffectBundle {\n  primary: HealEffect {\n    amount: 0,\n    label: \"\",\n  },\n  additional: [\n    HealEffect {\n      amount: 5,\n      label: \"Recovery\",\n    },\n    HealEffect {\n      amount: 5,\n      label: \"Recovery\",\n    }\n  ],\n}\n"
    );
}

#[test]
fn formatter_normalizes_blank_lines_and_safe_inline_spacing() {
    let source = "\n\n  type   Calculator   {   \n\n\n\
add :fn ( left : int,right:  int )->int ;   \n\
label: string = \"a  #  b\"; # keep  comment   \n\n\n\
}  \n\n";

    assert_eq!(
        format_cft(source),
        "type Calculator {\n  add: fn(left: int, right: int) -> int;\n  label: string = \"a  #  b\"; # keep  comment\n}\n"
    );

    assert_eq!(
        format_cft("check Rules {\nvalue>=10&&value!=20;\n}"),
        "check Rules {\n  value >= 10 && value != 20;\n}\n"
    );
    assert_eq!(
        format_cft("type Child:Parent {\nvalue:int;\n}"),
        "type Child : Parent {\n  value: int;\n}\n"
    );
    assert_eq!(
        format_cft("check Rules {\nvalue+offset*-1;\nvalue//2>0;\n}"),
        "check Rules {\n  value + offset * -1;\n  value // 2 > 0;\n}\n"
    );
}

#[test]
fn formatter_isolates_annotated_fields_as_one_definition() {
    let source = "type Item {\n\
@label ( \"Name\" )\n\n\
name :string;\n\
count:int;\n\
@description(\"Shown in the editor\")\n\
enabled:bool;\n\
}\n";

    assert_eq!(
        format_cft(source),
        "type Item {\n\n  @label(\"Name\")\n  name: string;\n\n  count: int;\n\n  @description(\"Shown in the editor\")\n  enabled: bool;\n}\n"
    );

    let previously_broken = "type Product {\n\n  @label(\"Name\")\n  name: string;\n\n  @label(\"Price\")\n\n  price: int;\n\n  @label(\"Enabled\")\n\n  enabled: bool;\n}\n";
    let repaired = "type Product {\n\n  @label(\"Name\")\n  name: string;\n\n  @label(\"Price\")\n  price: int;\n\n  @label(\"Enabled\")\n  enabled: bool;\n}\n";
    assert_eq!(format_cft(previously_broken), repaired);
    assert_eq!(format_cft(repaired), repaired);
}

#[test]
fn formatter_separates_top_level_definitions_but_keeps_their_annotations_attached() {
    let source = "enum Rarity {\nCommon,\n}\n\
@label(\"Product\")\n\
type Product {\nname:string;\n}\n\
const LIMIT:int=10;\n\
check Rules {\nLIMIT>0;\n}\n";

    assert_eq!(
        format_cft(source),
        "enum Rarity {\n  Common,\n}\n\n@label(\"Product\")\ntype Product {\n  name: string;\n}\n\nconst LIMIT: int = 10;\n\ncheck Rules {\n  LIMIT > 0;\n}\n"
    );
}

#[test]
fn formatter_rejoins_split_function_type_headers() {
    let source = "type Calculator {\nclassify:\n  fn(value: int) ->\n  string;\n}\n";
    assert_eq!(
        format_cft(source),
        "type Calculator {\n  classify: fn(value: int) -> string;\n}\n"
    );
}

#[test]
fn formatter_recovers_fields_and_indents_expression_continuations() {
    let source = "type Calculator {\nname:\nstring;\napply:\nfn(\nvalue: int\n)\n->\nResult<\nint,\nstring\n>;\n}\ncheck Rules {\nenabled && # continue after this comment\ncount >\n0;\nmatches(\nvalue,\n\"x\"\n);\n}\n";
    let expected = "type Calculator {\n  name: string;\n  apply: fn(\n    value: int\n  ) -> Result<\n    int,\n    string\n  >;\n}\n\ncheck Rules {\n  enabled && # continue after this comment\n    count >\n    0;\n  matches(\n    value,\n    \"x\"\n  );\n}\n";
    assert_eq!(format_cft(source), expected);
    assert_eq!(format_cft(expected), expected);
}

#[test]
fn formatter_matches_the_canonical_whitespace_example() {
    let source = "\n\n\
type   Item:Base{\n\
idRef :&Item;\n\
tags:[ string ];\n\
lookup : { string :int};\n\n\n\
@label ( \"Display Name\" )\n\
@description(\"Shown in editor\")\n\n\
name :string;\n\
enabled:bool;\n\n\n\
calculate :fn (value:int)->int;\n\n\
check{\n\
enabled&&calculate ( 10 )>=10;\n\
calculate(- 1)!=- 1;    # keep  comment\n\
}\n\
}\n\n";

    let expected = "type Item : Base {\n  idRef: &Item;\n  tags: [string];\n  lookup: {string: int};\n\n  @label(\"Display Name\")\n  @description(\"Shown in editor\")\n  name: string;\n\n  enabled: bool;\n\n  calculate: fn(value: int) -> int;\n\n  check {\n    enabled && calculate(10) >= 10;\n    calculate(-1) != -1; # keep  comment\n  }\n}\n";
    assert_eq!(format_cft(source), expected);
    assert_eq!(format_cft(expected), expected);

    assert_eq!(
        format_cft("type Box {\nvalue:Result<Option<int>,string>;\ncheck { a>b; c<d; }\n}"),
        "type Box {\n  value: Result<Option<int>, string>;\n\n  check {\n    a > b;\n    c < d;\n  }\n}\n"
    );
    assert_eq!(
        format_cft("type Callback=fn(value:int)->Result<int,string>;"),
        "type Callback = fn(value: int) -> Result<int, string>;\n"
    );
}

#[test]
fn formatter_attaches_standalone_opening_braces() {
    let source = "type Item\n\
\n\
{\n\
@label(\"Details\")\n\
details: Details\n\
{\n\
value:int;\n\
}\n\
enabled:bool;\n\
check\n\
{\n\
enabled;\n\
}\n\
}\n";

    assert_eq!(
        format_cft(source),
        "type Item {\n\n  @label(\"Details\")\n  details: Details {\n    value: int;\n  }\n\n  enabled: bool;\n  check {\n    enabled;\n  }\n}\n"
    );
}
