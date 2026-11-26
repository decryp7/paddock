use super::declared::{Kind, Scope};
use super::sccache::*;
use super::*;
use paddock_env::MapVars;

#[test]
fn the_table_is_sorted_named_and_consistent() {
    let names: Vec<&str> = DECLARED.iter().map(|d| d.name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        names, sorted,
        "DECLARED is sorted by name with no repeat (declaration() searches it)"
    );
    for d in DECLARED {
        assert!(d.name.starts_with("PADDOCK_"), "{}", d.name);
        assert!(
            !d.what.is_empty() && d.what.ends_with(['.', ')']),
            "{} needs one sentence",
            d.name
        );
        assert_eq!(
            d.kind.is_secret(),
            d.scope == Scope::Secret,
            "{}: a secret kind and a secret scope go together",
            d.name
        );
        assert!(
            !(d.kind.is_secret() && d.default.is_some()),
            "{}: a secret has no default",
            d.name
        );
        if d.kind == Kind::Number {
            assert!(
                d.default
                    .is_none_or(|v| v.bytes().all(|b| b.is_ascii_digit())),
                "{}",
                d.name
            );
        }
    }
    assert_eq!(
        declaration("PADDOCK_CI_MIN_FREE_GB").map(|d| d.default),
        Some(Some("30"))
    );
    assert!(declaration("PADDOCK_NOT_A_THING").is_none());
}

#[test]
fn a_number_is_a_u64_and_a_bad_one_falls_back_to_the_default() {
    let d = declaration("PADDOCK_CI_MIN_FREE_GB").unwrap();
    let get = |v: &str| Settings::new(&MapVars::new().with(d.name, v)).number_or_default(d);
    assert_eq!(get("45"), Some(45));
    assert_eq!(get(" 45 "), Some(45), "trimmed");
    assert_eq!(get("4x"), Some(30), "not digits: the default");
    assert_eq!(get(""), Some(30));
    assert_eq!(Settings::new(&MapVars::new()).number(d), None);
}

#[test]
fn a_switch_is_on_unless_empty_zero_or_false_and_an_exact_one_only_for_its_value() {
    let sw = declaration("PADDOCK_CI_RELEASE").unwrap();
    let on = |d: &Decl, v: &str| Settings::new(&MapVars::new().with(d.name, v)).is_on(d);
    assert!(on(sw, "1") && on(sw, "yes"));
    assert!(!on(sw, "0") && !on(sw, "false") && !on(sw, ""));
    assert!(!Settings::new(&MapVars::new()).is_on(sw));
    let ex = declaration("PADDOCK_TAG_REHEARSAL").unwrap();
    assert!(
        on(ex, "1") && !on(ex, "yes") && !on(ex, "true"),
        "Exact(\"1\") reads only 1"
    );
    let ssl = declaration("PADDOCK_SCCACHE_S3_SSL").unwrap();
    assert!(on(ssl, "true") && !on(ssl, "1"));
}

#[test]
fn a_secret_never_prints_and_a_bad_one_is_refused_without_its_value() {
    let d = declaration("PADDOCK_TEST_CACHE_SECRET").unwrap();
    let ok = Settings::new(&MapVars::new().with(d.name, " hunter2-secret "))
        .secret(d)
        .unwrap()
        .unwrap();
    assert_eq!(ok.expose(), "hunter2-secret");
    assert!(!format!("{ok:?} {ok}").contains("hunter2"));
    assert_eq!(Settings::new(&MapVars::new()).secret(d), Ok(None));
    assert_eq!(
        Settings::new(&MapVars::new().with(d.name, "  ")).secret(d),
        Ok(None)
    );
    let e = Settings::new(&MapVars::new().with(d.name, "two\nlines-hunter2"))
        .secret(d)
        .unwrap_err();
    assert!(e.0.contains(d.name) && !e.0.contains("hunter2"), "{e}");
}

#[test]
fn layered_environments_read_the_first_that_holds_the_name() {
    let (a, b) = (
        MapVars::new().with("A", "1"),
        MapVars::new().with("A", "2").with("B", "3"),
    );
    let l = Layered(&[&a, &b]);
    assert_eq!(
        (l.get("A"), l.get("B"), l.get("C")),
        (Some("1".into()), Some("3".into()), None)
    );
    let map = HashMap::from([("K".to_string(), "v".to_string())]);
    assert_eq!(EnvMap(&map).get("K"), Some("v".into()));
}

#[test]
fn the_endpoint_is_read_once_and_says_what_the_variable_should_hold() {
    assert_eq!(parse_endpoint("http://h:1"), Ok((false, "h:1".into())));
    assert_eq!(parse_endpoint("https://h:1/"), Ok((true, "h:1".into())));
    assert_eq!(
        parse_endpoint("h:1"),
        Ok((false, "h:1".into())),
        "a bare host:port is plain HTTP"
    );
    let at = parse_endpoint("http://user:pw@h:1").unwrap_err();
    assert!(at.contains('@') && !at.contains("pw"), "{at}");
    for bad in ["http://h:1/path", "h", "http://http://h:1"] {
        assert!(
            parse_endpoint(bad)
                .unwrap_err()
                .contains("http://host:port"),
            "{bad}"
        );
    }
}

fn ci_vars(extra: &[(&str, &str)]) -> MapVars {
    let mut v = MapVars::new()
        .with("PADDOCK_SCCACHE_S3_ACCESS_KEY_ID", "test-id")
        .with("PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY", "test-secret")
        .with("PADDOCK_SCCACHE_S3_BUCKET", "bkt")
        .with("PADDOCK_SCCACHE_S3_ENDPOINT", "http://s3.test:9000");
    for (k, x) in extra {
        v = v.with(k, *x);
    }
    v
}

#[test]
fn the_ci_level_is_built_from_the_variables_and_its_key_never_prints() {
    let s3 = S3Ci::from_vars(&ci_vars(&[]), "Linux", "aarch64").unwrap();
    assert_eq!(
        (s3.bucket.as_str(), s3.endpoint.as_str(), s3.ssl),
        ("bkt", "s3.test:9000", false)
    );
    assert_eq!(s3.prefix, "paddock-ci/Linux-aarch64");
    assert_eq!(
        (s3.key.0.expose(), s3.key.1.expose()),
        ("test-id", "test-secret")
    );
    assert!(!format!("{s3:?}").contains("test-secret") && !format!("{s3:?}").contains("test-id"));
}

#[test]
fn a_release_a_tag_or_a_rehearsal_never_reads_the_shared_store() {
    for (k, v) in [
        ("GITHUB_REF", "refs/tags/v1"),
        ("PADDOCK_TAG_REHEARSAL", "1"),
        ("PADDOCK_CI_RELEASE", "yes"),
    ] {
        let off = S3Ci::from_vars(&ci_vars(&[(k, v)]), "Linux", "x86_64").unwrap_err();
        assert!(
            !off.warn && off.why.contains("never reads the shared S3 cache"),
            "{k}: {off:?}"
        );
    }
    assert!(
        S3Ci::from_vars(
            &ci_vars(&[
                ("PADDOCK_CI_RELEASE", "0"),
                ("PADDOCK_TAG_REHEARSAL", "yes")
            ]),
            "Linux",
            "x86_64"
        )
        .is_ok()
    );
}

#[test]
fn a_missing_key_half_or_variable_says_which_and_how_loudly() {
    let none = S3Ci::from_vars(&MapVars::new(), "Linux", "x86_64").unwrap_err();
    assert!(!none.warn && none.why.contains("passes no S3 key"));
    let no_id = MapVars::new().with("PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY", "s");
    assert!(
        S3Ci::from_vars(&no_id, "Linux", "x86_64")
            .unwrap_err()
            .why
            .contains("PADDOCK_SCCACHE_S3_ACCESS_KEY_ID")
    );
    let no_secret = MapVars::new().with("PADDOCK_SCCACHE_S3_ACCESS_KEY_ID", "i");
    assert!(
        S3Ci::from_vars(&no_secret, "Linux", "x86_64")
            .unwrap_err()
            .why
            .contains("PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY")
    );
    let keys = MapVars::new()
        .with("PADDOCK_SCCACHE_S3_ACCESS_KEY_ID", "i")
        .with("PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY", "s");
    let off = S3Ci::from_vars(&keys, "Linux", "x86_64").unwrap_err();
    assert!(
        off.warn && off.why.contains("PADDOCK_SCCACHE_S3_BUCKET"),
        "{off:?}"
    );
    let bad = S3Ci::from_vars(
        &ci_vars(&[("PADDOCK_SCCACHE_S3_ENDPOINT", "http://u:p@h:1")]),
        "Linux",
        "x86_64",
    )
    .unwrap_err();
    assert!(bad.warn && !bad.why.contains(":p@"));
}

#[test]
fn the_store_endpoint_falls_back_to_the_declared_default() {
    let e = |v: MapVars| s3_endpoint(&Settings::new(&v));
    assert_eq!(e(MapVars::new()), Ok((false, "172.16.1.220:15000".into())));
    assert_eq!(
        e(MapVars::new().with("PADDOCK_SCCACHE_S3_ENDPOINT", "https://x:2")),
        Ok((true, "x:2".into()))
    );
    assert!(e(MapVars::new().with("PADDOCK_SCCACHE_S3_ENDPOINT", "x:2/p")).is_err());
}

#[test]
fn the_test_cache_address_takes_the_scheme_from_the_endpoint_and_never_adds_it_twice() {
    let a = |v: MapVars| test_cache_address(&Settings::new(&v));
    assert_eq!(
        a(MapVars::new()).unwrap(),
        TestCacheAddress {
            base: "http://172.16.1.220:15000".into(),
            region: "auto".into()
        }
    );
    let tls = a(MapVars::new()
        .with("PADDOCK_SCCACHE_S3_ENDPOINT", "https://h:1")
        .with("PADDOCK_SCCACHE_S3_REGION", "eu"))
    .unwrap();
    assert_eq!(
        (tls.base.as_str(), tls.region.as_str()),
        ("https://h:1", "eu")
    );
    let ssl = a(MapVars::new()
        .with("PADDOCK_SCCACHE_S3_ENDPOINT", "h:1")
        .with("PADDOCK_SCCACHE_S3_SSL", "true"))
    .unwrap();
    assert_eq!(
        ssl.base, "https://h:1",
        "the SSL variable turns TLS on for a bare host:port"
    );
    assert!(a(MapVars::new().with("PADDOCK_SCCACHE_S3_ENDPOINT", "http://http://h:1")).is_err());
}

#[test]
fn the_test_cache_key_needs_both_halves() {
    let k = |v: MapVars| test_cache_key(&Settings::new(&v));
    assert_eq!(k(MapVars::new()), Ok(None));
    assert_eq!(
        k(MapVars::new().with("PADDOCK_TEST_CACHE_KEY_ID", "i")),
        Ok(None),
        "one half is no key: the reader falls back"
    );
    let both = k(MapVars::new()
        .with("PADDOCK_TEST_CACHE_KEY_ID", "i")
        .with("PADDOCK_TEST_CACHE_SECRET", "s"))
    .unwrap()
    .unwrap();
    assert_eq!((both.0.expose(), both.1.expose()), ("i", "s"));
}

#[test]
fn the_server_settings_carry_the_chain_the_bucket_the_endpoint_tls_and_the_prefix() {
    let v: HashMap<_, _> = multilevel_settings("/d", "1G", "b", "h:1", true, "p/x")
        .into_iter()
        .collect();
    assert_eq!(v["SCCACHE_MULTILEVEL_CHAIN"], "disk,s3");
    assert_eq!(v["SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY"], "l0");
    assert_eq!(
        (
            v["SCCACHE_BUCKET"].as_str(),
            v["SCCACHE_ENDPOINT"].as_str(),
            v["SCCACHE_S3_USE_SSL"].as_str()
        ),
        ("b", "h:1", "true")
    );
    assert_eq!(
        (
            v["SCCACHE_S3_KEY_PREFIX"].as_str(),
            v["SCCACHE_DIR"].as_str(),
            v["SCCACHE_CACHE_SIZE"].as_str()
        ),
        ("p/x", "/d", "1G")
    );
    assert_eq!(
        multilevel_settings("/d", "1G", "b", "h:1", false, "p")
            .iter()
            .find(|(k, _)| k == "SCCACHE_S3_USE_SSL")
            .unwrap()
            .1,
        "false"
    );
}

#[test]
fn a_server_never_exits_on_its_own_and_with_s3_never_asks_instance_metadata() {
    assert_eq!(
        survival_vars(false),
        [("SCCACHE_IDLE_TIMEOUT".to_string(), "0".to_string())]
    );
    assert!(
        survival_vars(true)
            .contains(&("AWS_EC2_METADATA_DISABLED".to_string(), "true".to_string()))
    );
    assert_eq!(
        CI_KEY_ENV,
        [("CARGO_INCREMENTAL", "0"), ("CARGO_TERM_COLOR", "always")]
    );
}

#[test]
fn base_folders_are_absolute_once_each_and_the_manifest_paths_stay_verbatim() {
    let roots = ["/v/wt/a", "/v/wt/b/", "/v/wt/a", "rel/c", ""].map(String::from);
    let v = basedir_vars(&roots, false);
    assert_eq!(
        v[0],
        (
            "SCCACHE_BASEDIRS".to_string(),
            "/v/wt/a:/v/wt/b".to_string()
        )
    );
    assert_eq!(
        v[1],
        (
            "SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS".to_string(),
            "CARGO_MANIFEST_DIR,CARGO_MANIFEST_PATH".to_string()
        )
    );
    let w = basedir_vars(
        &[
            r"C:\ci\a".to_string(),
            "D:/t/x/".to_string(),
            "/c/git".to_string(),
        ],
        true,
    );
    assert_eq!(w[0].1, r"C:\ci\a;D:/t/x");
    assert!(
        basedir_vars(&["/c/only".to_string()], true).is_empty()
            && basedir_vars(&[], false).is_empty()
    );
}

#[test]
fn the_prefix_is_one_per_runner_class() {
    assert_eq!(ci_prefix("macOS", "aarch64"), "paddock-ci/macOS-aarch64");
}
