//! The one validity rule for a placement, used to drop entries on read, to check a layout and to refuse an edit.

use crate::catalogue::{Catalogue, Widget, spec_of};
use crate::error::WidgetError;
use crate::layout::Placement;

/// Whether `p` may be added after `existing` (which is already valid), with at most `max` placements in all.
pub(crate) fn check_one<C: Catalogue>(
    existing: &[Placement<C::Widget>],
    p: &Placement<C::Widget>,
    max: usize,
) -> Result<(), WidgetError> {
    p.widget.validate().map_err(WidgetError::Invalid)?;
    let kind = p.widget.kind();
    let Some(spec) = spec_of::<C>(kind) else {
        return Err(WidgetError::Invalid(
            "This widget is not available here.".into(),
        ));
    };
    if !spec.spans.contains(&p.span) {
        return Err(WidgetError::SpanNotAllowed {
            kind: spec.title.into(),
            spans: spec.spans.to_vec(),
        });
    }
    if existing.iter().any(|o| o.id == p.id) {
        return Err(WidgetError::DuplicateId(p.id.to_string()));
    }
    if existing.len() >= max {
        return Err(WidgetError::TooMany {
            what: "widgets".into(),
            max: u16::try_from(max).unwrap_or(u16::MAX),
        });
    }
    if existing.iter().filter(|o| o.widget.kind() == kind).count() >= spec.max_instances as usize {
        return Err(WidgetError::KindLimit {
            kind: spec.title.into(),
            max: spec.max_instances,
        });
    }
    Ok(())
}

/// Checks every placement in turn; the first problem is returned.
pub(crate) fn check_all<C: Catalogue>(
    placements: &[Placement<C::Widget>],
    max: usize,
) -> Result<(), WidgetError> {
    (0..placements.len()).try_for_each(|i| check_one::<C>(&placements[..i], &placements[i], max))
}

/// Clamps each widget and keeps the valid ones in order, counting the rest in `dropped`.
pub(crate) fn keep_valid<C: Catalogue>(
    items: Vec<Placement<C::Widget>>,
    max: usize,
    dropped: &mut u32,
) -> Vec<Placement<C::Widget>> {
    let mut kept: Vec<Placement<C::Widget>> = Vec::new();
    for mut p in items {
        p.widget = p.widget.clamped();
        if check_one::<C>(&kept, &p, max).is_ok() {
            kept.push(p);
        } else {
            *dropped = dropped.saturating_add(1);
        }
    }
    kept
}
