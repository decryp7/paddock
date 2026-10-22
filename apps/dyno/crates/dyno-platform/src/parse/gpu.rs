//! GPU utilisation from the `\GPU Engine(*)\Utilization Percentage` counter instances, named like
//! `pid_1234_luid_0x00000000_0x0000D1B5_phys_0_eng_3_engtype_3D`: one instance per process per engine.

/// The busiest engine: its utilisation summed over processes (0–100) and its engine type (e.g. "3D").
pub fn busiest_engine<'a>(instances: impl IntoIterator<Item = (&'a str, f64)>) -> Option<(f32, String)> {
    let mut engines: Vec<(String, String, f64)> = Vec::new();
    for (name, value) in instances {
        if !value.is_finite() || value < 0.0 {
            continue;
        }
        let Some((engine, kind)) = engine_of(name) else { continue };
        match engines.iter_mut().find(|(e, _, _)| *e == engine) {
            Some(slot) => slot.2 += value,
            None => engines.push((engine, kind, value)),
        }
    }
    engines
        .into_iter()
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(_, kind, v)| (v.min(100.0) as f32, kind))
}

/// The engine (adapter + physical index + engine index) and engine type of an instance name.
fn engine_of(name: &str) -> Option<(String, String)> {
    let luid = name.find("luid_")?;
    let (engine, kind) = name[luid..].split_once("_engtype_")?;
    Some((engine.to_string(), kind.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_processes_per_engine_and_takes_the_busiest() {
        let inst = [
            ("pid_1_luid_0x0_0xD1_phys_0_eng_0_engtype_3D", 20.0),
            ("pid_2_luid_0x0_0xD1_phys_0_eng_0_engtype_3D", 15.0),
            ("pid_1_luid_0x0_0xD1_phys_0_eng_3_engtype_VideoDecode", 30.0),
            ("pid_3_luid_0x0_0xD1_phys_0_eng_3_engtype_VideoDecode", f64::NAN),
            ("garbage", 99.0),
            ("pid_4_luid_0x0_0xD1_phys_0_eng_5_engtype_Copy", -1.0),
        ];
        assert_eq!(busiest_engine(inst), Some((35.0, "3D".into())));
        assert_eq!(busiest_engine([("pid_1_luid_a_phys_0_eng_0_engtype_3D", 70.0), ("pid_2_luid_a_phys_0_eng_0_engtype_3D", 60.0)]), Some((100.0, "3D".into())));
        assert_eq!(busiest_engine([]), None);
    }
}
