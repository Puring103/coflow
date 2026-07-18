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
}

impl CftCheckBuiltin {
    pub const ALL: [Self; 9] = [
        Self::Len,
        Self::Contains,
        Self::Unique,
        Self::Min,
        Self::Max,
        Self::Sum,
        Self::Keys,
        Self::Values,
        Self::Matches,
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
        }
    }

    /// Total arity, including the method receiver.
    #[must_use]
    pub const fn arity(self) -> usize {
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

    #[must_use]
    pub const fn method_arity(self) -> usize {
        self.arity().saturating_sub(1)
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
            ]
        );
        for (name, _) in entries {
            assert!(CftCheckBuiltin::by_name(name).is_some());
        }
        assert_eq!(CftCheckBuiltin::by_name("missing"), None);
    }
}
