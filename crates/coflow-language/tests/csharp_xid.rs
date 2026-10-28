#![allow(clippy::expect_used)]
//! Loader 生成表必须随语言侧 Unicode 版本一起更新。
#[test]
fn csharp_xid_table_matches_every_unicode_scalar() {
    let source = include_str!("../../../runtimes/csharp/Coflow.Loader/Cfd/CfdXid.Generated.cs");
    assert!(source.contains(&format!("Unicode {:?}", unicode_ident::UNICODE_VERSION)));
    for (name, predicate) in [
        ("Start", unicode_ident::is_xid_start as fn(char) -> bool),
        (
            "Continue",
            unicode_ident::is_xid_continue as fn(char) -> bool,
        ),
    ] {
        let table = source
            .split(&format!("{name}Ranges = {{"))
            .nth(1)
            .expect("table")
            .split("};")
            .next()
            .expect("table end");
        let bounds = table
            .split(',')
            .filter_map(|part| part.trim().strip_prefix("0x"))
            .map(|hex| u32::from_str_radix(hex, 16).expect("hex bound"))
            .collect::<Vec<_>>();
        assert_eq!(bounds.len() % 2, 0);
        let mut range = 0;
        for code in 0..=0x0010_ffff {
            while range < bounds.len() && code > bounds[range + 1] {
                range += 2;
            }
            let actual = range < bounds.len() && code >= bounds[range];
            assert_eq!(
                actual,
                char::from_u32(code).is_some_and(predicate),
                "{name}: U+{code:X}; regenerate with export_csharp_xid"
            );
        }
    }
}
