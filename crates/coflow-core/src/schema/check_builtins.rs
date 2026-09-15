#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CftCheckBuiltin {
    Len,
    Contains,
    Unique,
    Min,
    Max,
    Sum,
    Keys,
    Values,
    Matches,
    StartsWith,
    EndsWith,
    IsBlank,
    Abs,
    IsFinite,
    ApproxEqual,
    ContainsKey,
    ContainsValue,
    IsSorted,
    IsStrictlySorted,
    Intersects,
    IsDisjoint,
    IsSubsetOf,
    IsSupersetOf,
}

impl CftCheckBuiltin {
    pub const ALL: [Self; 23] = [
        Self::Len,
        Self::Contains,
        Self::Unique,
        Self::Min,
        Self::Max,
        Self::Sum,
        Self::Keys,
        Self::Values,
        Self::Matches,
        Self::StartsWith,
        Self::EndsWith,
        Self::IsBlank,
        Self::Abs,
        Self::IsFinite,
        Self::ApproxEqual,
        Self::ContainsKey,
        Self::ContainsValue,
        Self::IsSorted,
        Self::IsStrictlySorted,
        Self::Intersects,
        Self::IsDisjoint,
        Self::IsSubsetOf,
        Self::IsSupersetOf,
    ];

    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|builtin| builtin.name() == name)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Len => "len",
            Self::Contains => "contains",
            Self::Unique => "isUnique",
            Self::Min => "min",
            Self::Max => "max",
            Self::Sum => "sum",
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Matches => "matches",
            Self::StartsWith => "startsWith",
            Self::EndsWith => "endsWith",
            Self::IsBlank => "isBlank",
            Self::Abs => "abs",
            Self::IsFinite => "isFinite",
            Self::ApproxEqual => "approxEqual",
            Self::ContainsKey => "containsKey",
            Self::ContainsValue => "containsValue",
            Self::IsSorted => "isSorted",
            Self::IsStrictlySorted => "isStrictlySorted",
            Self::Intersects => "intersects",
            Self::IsDisjoint => "isDisjoint",
            Self::IsSubsetOf => "isSubsetOf",
            Self::IsSupersetOf => "isSupersetOf",
        }
    }

    /// Total arity, including the method receiver.
    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            Self::Contains
            | Self::Matches
            | Self::StartsWith
            | Self::EndsWith
            | Self::ContainsKey
            | Self::ContainsValue
            | Self::Intersects
            | Self::IsDisjoint
            | Self::IsSubsetOf
            | Self::IsSupersetOf => 2,
            Self::ApproxEqual => 3,
            Self::Len
            | Self::Unique
            | Self::Min
            | Self::Max
            | Self::Sum
            | Self::Keys
            | Self::Values
            | Self::IsBlank
            | Self::Abs
            | Self::IsFinite
            | Self::IsSorted
            | Self::IsStrictlySorted => 1,
        }
    }

    #[must_use]
    pub const fn method_arity(self) -> usize {
        self.arity().saturating_sub(1)
    }

    #[must_use]
    pub const fn documentation(self) -> &'static str {
        match self {
            Self::Len => "value.len(): return the number of string characters or collection items.",
            Self::Contains => {
                "value.contains(val): test string substring, array element, or dict key presence."
            }
            Self::Unique => "value.isUnique(): true when supported scalar elements are unique.",
            Self::Min => "value.min(): minimum value in a non-empty int, float, or enum array.",
            Self::Max => "value.max(): maximum value in a non-empty int, float, or enum array.",
            Self::Sum => "value.sum(): sum an int or float array.",
            Self::Keys => "value.keys(): return dict keys as an array.",
            Self::Values => "value.values(): return dict values as an array.",
            Self::Matches => "value.matches(pattern): match a static regular expression.",
            Self::StartsWith => "value.startsWith(prefix): test a string prefix.",
            Self::EndsWith => "value.endsWith(suffix): test a string suffix.",
            Self::IsBlank => {
                "value.isBlank(): test whether a string is empty or Unicode whitespace."
            }
            Self::Abs => "value.abs(): return the absolute int or float value.",
            Self::IsFinite => "value.isFinite(): test whether a float is finite.",
            Self::ApproxEqual => {
                "value.approxEqual(other, epsilon): compare floats by absolute error."
            }
            Self::ContainsKey => "value.containsKey(key): test dict key presence.",
            Self::ContainsValue => "value.containsValue(value): test dict value presence.",
            Self::IsSorted => "value.isSorted(): test non-decreasing array order.",
            Self::IsStrictlySorted => {
                "value.isStrictlySorted(): test strictly increasing array order."
            }
            Self::Intersects => "value.intersects(other): test whether arrays share an element.",
            Self::IsDisjoint => "value.isDisjoint(other): test whether arrays share no elements.",
            Self::IsSubsetOf => "value.isSubsetOf(other): test mathematical subset membership.",
            Self::IsSupersetOf => {
                "value.isSupersetOf(other): test mathematical superset membership."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_keeps_names_and_arities_together() {
        let entries = CftCheckBuiltin::ALL
            .iter()
            .map(|builtin| (builtin.name(), builtin.arity()))
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            vec![
                ("len", 1),
                ("contains", 2),
                ("isUnique", 1),
                ("min", 1),
                ("max", 1),
                ("sum", 1),
                ("keys", 1),
                ("values", 1),
                ("matches", 2),
                ("startsWith", 2),
                ("endsWith", 2),
                ("isBlank", 1),
                ("abs", 1),
                ("isFinite", 1),
                ("approxEqual", 3),
                ("containsKey", 2),
                ("containsValue", 2),
                ("isSorted", 1),
                ("isStrictlySorted", 1),
                ("intersects", 2),
                ("isDisjoint", 2),
                ("isSubsetOf", 2),
                ("isSupersetOf", 2),
            ]
        );
        for (name, _) in entries {
            assert!(CftCheckBuiltin::by_name(name).is_some());
        }
        let names = CftCheckBuiltin::ALL
            .iter()
            .map(|builtin| builtin.name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), CftCheckBuiltin::ALL.len());
        assert!(CftCheckBuiltin::ALL
            .iter()
            .all(|builtin| !builtin.documentation().is_empty()));
        assert_eq!(CftCheckBuiltin::by_name("missing"), None);
    }
}
