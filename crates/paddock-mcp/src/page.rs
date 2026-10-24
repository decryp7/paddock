//! Cursors: opaque to the model, an offset to the server. Tools that return long lists use the same helpers as `tools/list`.

/// The cursor for the page that starts at `offset`.
pub fn encode(offset: usize) -> String {
    format!("p{offset:x}")
}

/// The offset a cursor stands for.
pub fn decode(cursor: &str) -> Result<usize, String> {
    cursor
        .strip_prefix('p')
        .and_then(|h| usize::from_str_radix(h, 16).ok())
        .ok_or_else(|| {
            format!("\"{cursor}\" is not a cursor; use the one the previous result gave")
        })
}

/// One page of `items` starting at `cursor` (or the start), at most `size` long, and the cursor of the next page when there is
/// one. A cursor past the end is an error: the list changed or the cursor is not from this list.
pub fn slice<'a, T>(
    items: &'a [T],
    cursor: Option<&str>,
    size: usize,
) -> Result<(&'a [T], Option<String>), String> {
    let start = cursor.map(decode).transpose()?.unwrap_or(0);
    if start > items.len() {
        return Err(format!(
            "the cursor is past the end of the list ({} items); start again without a cursor",
            items.len()
        ));
    }
    let end = start.saturating_add(size.max(1)).min(items.len());
    Ok((&items[start..end], (end < items.len()).then(|| encode(end))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_walk_the_whole_list_once() {
        let items: Vec<u32> = (0..7).collect();
        let (mut got, mut cursor) = (vec![], None::<String>);
        loop {
            let (page, next) = slice(&items, cursor.as_deref(), 3).unwrap();
            got.extend_from_slice(page);
            match next {
                Some(n) => cursor = Some(n),
                None => break,
            }
        }
        assert_eq!(got, items);
        assert_eq!(
            slice(&items, None, 0).unwrap().0.len(),
            1,
            "a page is never empty while items remain"
        );
        assert!(
            slice(&items, Some("p7"), 3).unwrap().0.is_empty(),
            "exactly at the end is an empty last page"
        );
    }

    #[test]
    fn bad_cursors_are_refused_with_what_to_do() {
        for bad in ["", "x1", "p", "pzz", "p-1"] {
            assert!(decode(bad).unwrap_err().contains("not a cursor"), "{bad}");
        }
        assert!(
            slice(&[1], Some("pff"), 3)
                .unwrap_err()
                .contains("past the end")
        );
        assert_eq!(decode(&encode(123_456)).unwrap(), 123_456);
    }
}
