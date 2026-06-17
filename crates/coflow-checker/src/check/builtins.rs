#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Builtin {
    Len,
    Contains,
    Unique,
    Min,
    Max,
    Sum,
    Keys,
    Values,
    Matches,
}

impl Builtin {
    pub(super) fn by_name(name: &str) -> Option<Self> {
        BUILTINS
            .iter()
            .copied()
            .find(|builtin| builtin.name() == name)
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Len => "len",
            Self::Contains => "contains",
            Self::Unique => "unique",
            Self::Min => "min",
            Self::Max => "max",
            Self::Sum => "sum",
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Matches => "matches",
        }
    }

    pub(super) const fn arity(self) -> usize {
        match self {
            Self::Contains | Self::Matches => 2,
            Self::Len
            | Self::Unique
            | Self::Min
            | Self::Max
            | Self::Sum
            | Self::Keys
            | Self::Values => 1,
        }
    }
}

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin::Len,
    Builtin::Contains,
    Builtin::Unique,
    Builtin::Min,
    Builtin::Max,
    Builtin::Sum,
    Builtin::Keys,
    Builtin::Values,
    Builtin::Matches,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_keeps_names_and_arities_together() {
        let entries = BUILTINS
            .iter()
            .map(|builtin| (builtin.name(), builtin.arity()))
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            vec![
                ("len", 1),
                ("contains", 2),
                ("unique", 1),
                ("min", 1),
                ("max", 1),
                ("sum", 1),
                ("keys", 1),
                ("values", 1),
                ("matches", 2),
            ]
        );
        for (name, _) in entries {
            assert!(Builtin::by_name(name).is_some());
        }
        assert_eq!(Builtin::by_name("missing"), None);
    }
}
