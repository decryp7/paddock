//! Packs are files users drop into a folder: no input makes the parser panic (conformance check C36).
use paddock_packs::{Limits, Origin, PackError, PackKind, parse_pack};
use paddock_testkit::assert_never_panics;

/// A kind with one list of names, enough to run the shared parse path.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
struct Names {
    #[serde(default)]
    names: Vec<String>,
}

struct Kind;
impl PackKind for Kind {
    type Raw = Names;
    type Pack = Names;
    const FORMAT: u32 = 1;
    fn convert(raw: Names, _: Origin) -> Result<Names, PackError> {
        Ok(raw)
    }
}

#[test]
fn packs_survive_hostile_bytes() {
    assert_never_panics(
        &[
            b"format = 1\nnames = [\"a\", \"b\"]\n",
            b"[table]\nkey = 1\n",
        ],
        |b| parse_pack::<Kind>(b, Origin::Builtin, &Limits::default()),
    );
}
