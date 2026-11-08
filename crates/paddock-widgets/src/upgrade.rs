//! The generic upgrade from span layouts to cell layouts (ADR 0044 §4).

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::catalogue::{Catalogue, Widget, spec_of};
use crate::grid::{MAX_ROWS, SizeRule};
use crate::place::Place;

/// One entry of a span layout, turned into a width and a default height.
struct Entry {
    id: Value,
    widget: Value,
    w: u8,
    h: u8,
    rule: SizeRule,
}

/// Reads one `{ id, span, widget }` entry; `None` when it cannot be upgraded (no widget of the catalogue, no span).
fn entry<C: Catalogue>(value: &Value) -> Option<Entry> {
    let object = value.as_object()?;
    let widget_value = object.get("widget")?;
    let widget = C::Widget::deserialize(widget_value).ok()?;
    let rule = <C::Place as Place>::size_rule(&spec_of::<C>(widget.kind())?.size)?;
    let span = u8::try_from(object.get("span")?.as_u64()?).ok()?;
    Some(Entry {
        id: object.get("id")?.clone(),
        widget: widget_value.clone(),
        w: rule.widths.nearest(span).min(C::SURFACE.columns).max(1),
        h: rule.default.h,
        rule,
    })
}

/// A list of span entries as cell entries: each in stored order gets its width and the kind's default height, goes to
/// the first place CSS grid's auto-placement gave it in 1.0 (left to right, a new row when it does not fit), then every
/// entry on one visual row gets the row's tallest height (1.0 stretched a row to its tallest card). Entries that cannot
/// be upgraded, or that do not fit in [`MAX_ROWS`], are left out.
fn cells<C: Catalogue>(entries: &[Value]) -> Vec<Value> {
    let columns = C::SURFACE.columns;
    let mut rows: Vec<Vec<(u8, Entry)>> = Vec::new();
    let mut col = 0u8;
    for e in entries.iter().filter_map(entry::<C>) {
        if rows.is_empty() || u16::from(col) + u16::from(e.w) > u16::from(columns) {
            rows.push(Vec::new());
            col = 0;
        }
        let x = col;
        col += e.w;
        if let Some(row) = rows.last_mut() {
            row.push((x, e));
        }
    }
    let mut out = Vec::new();
    let mut y = 0u16;
    for row in rows {
        let height = row.iter().map(|(_, e)| e.h).max().unwrap_or(1);
        if u32::from(y) + u32::from(height) > u32::from(MAX_ROWS) {
            break;
        }
        for (x, e) in row {
            out.push(json!({ "id": e.id, "x": x, "y": y, "w": e.w, "h": e.rule.heights.nearest(height), "widget": e.widget }));
        }
        y += u16::from(height);
    }
    out
}

/// Upgrades a version 1 span layout (`{ version, placements: [{ id, span, widget }], presets }`) to the surface's
/// current schema with cell placements, presets included. For a grid catalogue's [`Catalogue::upgrade`].
pub fn spans_to_cells<C: Catalogue>(value: Value) -> Result<Value, String> {
    let Value::Object(mut object) = value else {
        return Err("not a layout".into());
    };
    let Some(Value::Array(placements)) = object.get("placements") else {
        return Err("no placements".into());
    };
    let upgraded = cells::<C>(placements);
    object.insert("placements".into(), Value::Array(upgraded));
    if let Some(Value::Array(presets)) = object.get("presets") {
        let presets = presets
            .iter()
            .map(|p| match p {
                Value::Object(o) => {
                    let mut o: Map<String, Value> = o.clone();
                    if let Some(Value::Array(list)) = o.get("placements") {
                        o.insert("placements".into(), Value::Array(cells::<C>(list)));
                    }
                    Value::Object(o)
                }
                other => other.clone(),
            })
            .collect();
        object.insert("presets".into(), Value::Array(presets));
    }
    object.insert("version".into(), Value::from(C::SURFACE.schema_version));
    Ok(Value::Object(object))
}
