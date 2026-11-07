//! The TypeScript declarations the apps generate from: counts are `number`, wrappers are named like the wire.

use ts_rs::TS;

#[test]
fn declarations_use_number_for_counts_and_the_wire_names() {
    let quantity = crate::Quantity::decl();
    assert!(
        quantity.contains("number") && !quantity.contains("bigint"),
        "{quantity}"
    );
    let total = crate::Total::decl();
    assert!(
        total.contains("\"kind\": \"exact\"")
            && total.contains("number")
            && !total.contains("bigint"),
        "{total}"
    );
    let selection = crate::Selection::decl();
    assert!(
        selection.contains("\"kind\": \"topBy\"") && !selection.contains("bigint"),
        "{selection}"
    );
    let cut = crate::CutWire::decl();
    assert!(cut.starts_with("type Cut = "), "{cut}");
    assert!(
        cut.contains("limit: LimitId") && cut.contains("of: FieldPath") && !cut.contains("bigint"),
        "{cut}"
    );
    let bounded = crate::BoundedWire::<()>::decl();
    assert!(
        bounded.contains("type Bounded<T>") && bounded.contains("cuts: Array<Cut>"),
        "{bounded}"
    );
    assert!(
        crate::Limit::inline().contains("number") && crate::Buckets::inline().contains("number")
    );
    assert_eq!(crate::LimitId::decl(), "type LimitId = string;");
    assert_eq!(crate::SettingPath::decl(), "type SettingPath = string;");
    assert_eq!(crate::FieldPath::decl(), "type FieldPath = string;");
    assert!(crate::LimitExceeded::decl().contains("reached: Quantity | null"));
}

/// The hand-written declarations of the string and number newtypes answer every part of the trait the same way.
#[test]
fn the_newtype_declarations_agree_in_every_form() {
    fn all<T: TS>() -> [String; 5] {
        [
            T::name(),
            T::inline(),
            T::inline_flattened(),
            T::decl(),
            T::decl_concrete(),
        ]
    }
    for (parts, ty, name) in [
        (all::<crate::LimitId>(), "string", "LimitId"),
        (all::<crate::SettingPath>(), "string", "SettingPath"),
        (all::<crate::FieldPath>(), "string", "FieldPath"),
        (all::<crate::Limit>(), "number", "Limit"),
        (all::<crate::Buckets>(), "number", "Buckets"),
    ] {
        assert_eq!(parts[0], name);
        assert_eq!(parts[1], ty);
        assert_eq!(parts[2], ty);
        assert_eq!(parts[3], format!("type {name} = {ty};"));
        assert_eq!(parts[4], parts[3]);
    }
}
