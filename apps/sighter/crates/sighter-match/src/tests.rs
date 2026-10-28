use super::*;
use proptest::prelude::*;

/// A deterministic textured frame with 40x40 "icons" drawn at the given corners.
fn scene(w: u32, h: u32, icons: &[(u32, u32)]) -> Frame {
    let mut seed = 12345u32;
    let px = (0..w * h)
        .map(|_| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let v = 200 + ((seed >> 16) % 20);
            (v << 16) | (v << 8) | v
        })
        .collect();
    let mut f = Frame::new(w, h, px).unwrap();
    for &(x, y) in icons {
        f.fill(Rect::new(x, y, 40, 40), 0x2050c0);
        f.fill(Rect::new(x + 8, y + 8, 10, 24), 0xffffff);
        f.fill(Rect::new(x + 22, y + 8, 10, 10), 0xffd000);
    }
    f
}

fn strict() -> (Calibrator, Thresholds) {
    (Calibrator::conservative(Layer::Template), Thresholds::default())
}

#[test]
fn a_crop_shifted_by_7_px_is_found_at_the_right_point() {
    let f = scene(640, 400, &[(301, 177)]);
    // The crop starts 4 px up-left of the icon; another crop (7 px off) must be found where it was cut.
    let t = f.crop(Rect::new(294, 170, 48, 48)).unwrap();
    let m = find(&f, &t, None, &Params::default()).unwrap();
    assert_eq!((m[0].rect.x, m[0].rect.y), (294, 170));
    assert!(m[0].ncc > 0.99);
    let (c, th) = strict();
    let Resolution::Found(hit) = locate(&f, &t, None, &Params::default(), &c, &th).unwrap() else {
        panic!("not found")
    };
    assert_eq!(hit.rect, Rect::new(294, 170, 48, 48));
}

#[test]
fn a_theme_inverted_crop_is_not_found_not_a_low_score_hit() {
    let f = scene(640, 400, &[(301, 177)]);
    let t = f.crop(Rect::new(297, 173, 48, 48)).unwrap().inverted();
    let (c, th) = strict();
    assert_eq!(
        locate(&f, &t, None, &Params::default(), &c, &th).unwrap(),
        Resolution::NotFound { best: None }
    );
}

#[test]
fn two_identical_icons_are_ambiguous() {
    let f = scene(500, 300, &[(20, 30), (400, 200)]);
    let t = scene(60, 60, &[(10, 10)]).crop(Rect::new(6, 6, 48, 48)).unwrap();
    let m = find(&f, &t, None, &Params::default()).unwrap();
    assert_eq!(m.len(), 2, "{m:?}");
    assert!(m.iter().any(|m| m.rect.x == 16 && m.rect.y == 26));
    let (c, th) = strict();
    assert!(matches!(
        locate(&f, &t, None, &Params::default(), &c, &th).unwrap(),
        Resolution::Ambiguous(_)
    ));
    // Restricting the search to one half makes it a clear find.
    let left = Some(Rect::new(0, 0, 250, 300));
    let Resolution::Found(hit) = locate(&f, &t, left, &Params::default(), &c, &th).unwrap() else {
        panic!("not found")
    };
    assert_eq!((hit.rect.x, hit.rect.y), (16, 26));
    let right = find(&f, &t, Some(Rect::new(300, 100, 500, 500)), &Params::default()).unwrap();
    assert_eq!(
        (right[0].rect.x, right[0].rect.y),
        (396, 196),
        "region hits are in frame coordinates"
    );
}

#[test]
fn small_templates_use_the_full_resolution_scan() {
    let f = scene(200, 120, &[(50, 60)]);
    let t = f.crop(Rect::new(56, 66, 20, 20)).unwrap();
    let m = find(
        &f,
        &t,
        None,
        &Params {
            min_raw: 0.95,
            ..Params::default()
        },
    )
    .unwrap();
    assert_eq!((m[0].rect.x, m[0].rect.y), (56, 66));
}

#[test]
fn bad_inputs_are_typed_errors() {
    let f = scene(300, 200, &[]);
    let t = scene(60, 60, &[(10, 10)]).crop(Rect::new(6, 6, 48, 48)).unwrap();
    assert!(find(&f, &t, None, &Params::default()).unwrap().is_empty());
    let flat = Frame::solid(20, 20, 0x808080).unwrap();
    assert_eq!(find(&f, &flat, None, &Params::default()), Err(MatchError::TemplateFlat));
    assert!(matches!(
        find(&t, &f, None, &Params::default()),
        Err(MatchError::TemplateTooLarge { .. })
    ));
    assert!(matches!(
        find(&f, &t, Some(Rect::new(900, 900, 5, 5)), &Params::default()),
        Err(MatchError::RegionOutside(_))
    ));
    assert!(matches!(
        find(&f, &t, Some(Rect::new(0, 0, 30, 30)), &Params::default()),
        Err(MatchError::TemplateTooLarge { .. })
    ));
    let tight = Params {
        work_budget: 1000,
        ..Params::default()
    };
    let err = find(&f, &t, None, &tight).unwrap_err();
    assert!(matches!(err, MatchError::TooMuchWork { budget: 1000, .. }));
    assert!(err.to_string().contains("budget"));
    assert!(MatchError::Calibration(ScoreError::NotFinite).to_string().contains("calibration"));
}

#[test]
fn a_wrong_calibrator_is_reported() {
    let f = scene(200, 120, &[(50, 60)]);
    let t = f.crop(Rect::new(56, 66, 20, 20)).unwrap();
    let wrong = Calibrator::conservative(Layer::Ocr);
    assert!(matches!(
        locate(&f, &t, None, &Params::default(), &wrong, &Thresholds::default()),
        Err(MatchError::Calibration(_))
    ));
}

#[test]
fn hit_limits_bound_the_result() {
    let f = scene(500, 300, &[(20, 30), (400, 200)]);
    let t = scene(60, 60, &[(10, 10)]).crop(Rect::new(6, 6, 48, 48)).unwrap();
    assert_eq!(
        find(
            &f,
            &t,
            None,
            &Params {
                max_hits: 1,
                ..Params::default()
            }
        )
        .unwrap()
        .len(),
        1
    );
    // A scan that stops after one raw position still returns a (possibly partial) deterministic answer.
    let capped = Params {
        max_raw_hits: 1,
        ..Params::default()
    };
    assert_eq!(find(&f, &t, None, &capped).unwrap(), find(&f, &t, None, &capped).unwrap());
    assert!(find(&f, &t, None, &capped).unwrap().len() <= 1);
    assert_eq!(ncc::DEFAULT_MAX_RAW_HITS, 65_536);
}

#[test]
fn the_work_budget_default_is_pinned() {
    assert_eq!(Params::default().work_budget, ncc::DEFAULT_WORK_BUDGET);
    assert_eq!(ncc::DEFAULT_WORK_BUDGET, 2_000_000_000);
}

#[test]
fn a_full_hd_frame_with_a_typical_icon_fits_the_default_budget() {
    // Pure arithmetic through the real check: a 1920x1080 frame and a 48x48 template need far less than the budget.
    let f = Frame::solid(1920, 1080, 0x808080).unwrap();
    let t = scene(60, 60, &[(10, 10)]).crop(Rect::new(6, 6, 48, 48)).unwrap();
    // A flat frame yields no hits, but only after the budget check passed and the scan ran at the coarse level.
    assert_eq!(find(&f, &t, None, &Params::default()).unwrap(), vec![]);
}

proptest! {
    /// A template that contains an icon (so it has edges to correlate; a template of pure texture is outside the layer's
    /// contract) is found where it was cut, at any alignment of the coarse level.
    #[test]
    fn a_cut_template_with_detail_is_found_where_it_was_cut(x in 5u32..30, y in 0u32..20, dw in 0u32..12, dh in 0u32..12) {
        let f = scene(120, 90, &[(30, 20)]);
        let t = f.crop(Rect::new(x, y, 70 - x + dw, 60 - y + dh)).unwrap();
        let m = find(&f, &t, None, &Params { min_raw: 0.9, ..Params::default() }).unwrap();
        prop_assert!(m.iter().any(|m| m.rect.x == x && m.rect.y == y), "{m:?}");
    }
}
