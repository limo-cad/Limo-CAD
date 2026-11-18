use super::*;

struct Fixture(Store);
impl Fixture {
    fn new() -> Self {
        Self(
            Store::at(
                std::env::temp_dir()
                    .join(format!("limo-cad-app-preferences-{}", uuid::Uuid::new_v4())),
            )
            .unwrap(),
        )
    }
    fn write(&self, text: impl AsRef<[u8]>) {
        fs::create_dir_all(self.0.path.parent().unwrap()).unwrap();
        fs::write(self.0.path(), text).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.path.parent().unwrap());
    }
}

#[test]
fn defaults_do_not_create_files_and_explicit_choices_persist() {
    let fixture = Fixture::new();
    let saved = fixture.0.read().unwrap();
    assert_eq!(saved, Preferences::default());
    assert_eq!(
        saved.effective(Locale::ZhCn),
        Effective {
            theme: ThemePreference::System,
            locale: Locale::ZhCn,
            six_dof_speed: 1.5,
            ui_scale: 1.,
            gpu_stock_removal: true,
            kernel: KernelPreference::Occt,
        }
    );
    assert!(!fixture.0.path.parent().unwrap().exists());
    fixture.0.patch(Preferences::default()).unwrap();
    assert!(!fixture.0.path.parent().unwrap().exists());
    let imported = fixture
        .0
        .patch(Preferences {
            theme: Some(ThemePreference::Dark),
            locale: Some(Locale::Es),
            six_dof_speed: Some(2.25),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(imported.theme, Some(ThemePreference::Dark));
    assert_eq!(imported.locale, Some(Locale::Es));
    assert_eq!(imported.six_dof_speed, Some(2.25));
    assert_eq!(fixture.0.read().unwrap(), imported);
}

#[test]
fn field_patches_retain_unedited_choices_and_unknown_metadata() {
    let fixture = Fixture::new();
    fixture.write(br#"{"schema_version":1,"theme":"system","future":{"keep":[1,2,3]}}"#);
    let first = fixture
        .0
        .patch(Preferences {
            locale: Some(Locale::ZhCn),
            six_dof_speed: Some(1.75),
            gpu_stock_removal: Some(false),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first.theme, Some(ThemePreference::System));
    let changed = fixture
        .0
        .patch(Preferences {
            locale: Some(Locale::De),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(changed.theme, first.theme);
    assert_eq!(changed.six_dof_speed, first.six_dof_speed);
    assert_eq!(changed.gpu_stock_removal, Some(false));
    assert!(
        !fixture
            .0
            .read()
            .unwrap()
            .effective(Locale::En)
            .gpu_stock_removal
    );
    assert_eq!(changed.locale, Some(Locale::De));
    let exact_before = fs::read(fixture.0.path()).unwrap();
    fixture.0.patch(Preferences::default()).unwrap();
    assert_eq!(fs::read(fixture.0.path()).unwrap(), exact_before);
    let raw: serde_json::Value = serde_json::from_slice(&exact_before).unwrap();
    assert_eq!(raw["future"], serde_json::json!({"keep":[1,2,3]}));
}

#[test]
fn speed_values_match_the_native_clamp_contract() {
    for (input, expected) in [(-2., 0.25), (0.25, 0.25), (2.1, 2.1), (5., 3.)] {
        assert_eq!(clamp_six_dof_speed(input), expected);
    }
    assert_eq!(clamp_six_dof_speed(f64::NAN), 1.5);
    assert_eq!(clamp_six_dof_speed(f64::INFINITY), 1.5);
    let fixture = Fixture::new();
    assert_eq!(
        fixture
            .0
            .patch(Preferences {
                six_dof_speed: Some(50.),
                ..Default::default()
            })
            .unwrap()
            .six_dof_speed,
        Some(3.)
    );
    assert_eq!(ThemePreference::System.resolve(true), ResolvedTheme::Dark);
    assert_eq!(ThemePreference::System.resolve(false), ResolvedTheme::Light);
    assert_eq!(ThemePreference::Light.resolve(true), ResolvedTheme::Light);
    assert_eq!(ThemePreference::Dark.resolve(false), ResolvedTheme::Dark);
}

#[test]
fn corrupt_oversized_or_newer_saved_preferences_are_not_replaced_by_a_patch() {
    let fixture = Fixture::new();
    for text in [
        "not json".into(),
        r#"{"schema_version":2,"theme":"dark"}"#.into(),
        r#"{"schema_version":1,"theme":"auto"}"#.into(),
        r#"{"schema_version":1,"locale":"en-GB"}"#.into(),
        r#"{"schema_version":1,"six_dof_speed":99}"#.into(),
        " ".repeat(MAX_BYTES as usize + 1),
    ] {
        fixture.write(&text);
        assert!(fixture.0.read().is_err());
        assert!(fixture
            .0
            .patch(Preferences {
                theme: Some(ThemePreference::Light),
                ..Default::default()
            })
            .is_err());
        assert_eq!(fs::read(fixture.0.path()).unwrap(), text.as_bytes());
    }
}

#[test]
fn independently_opened_writers_merge_fields_and_busy_os_lock_preserves_bytes() {
    let fixture = Fixture::new();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let joins: Vec<_> = [
        Preferences {
            theme: Some(ThemePreference::Dark),
            ..Default::default()
        },
        Preferences {
            locale: Some(Locale::Es),
            ..Default::default()
        },
    ]
    .into_iter()
    .map(|patch| {
        let store = fixture.0.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.patch(patch).unwrap();
        })
    })
    .collect();
    barrier.wait();
    for join in joins {
        join.join().unwrap();
    }
    let merged = fixture.0.read().unwrap();
    assert_eq!(merged.theme, Some(ThemePreference::Dark));
    assert_eq!(merged.locale, Some(Locale::Es));

    let bytes = fs::read(fixture.0.path()).unwrap();
    let other_process = OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.0.path.parent().unwrap().join(LOCK_NAME))
        .unwrap();
    other_process.try_lock().unwrap();
    assert!(fixture
        .0
        .patch(Preferences {
            six_dof_speed: Some(2.),
            ..Default::default()
        })
        .unwrap_err()
        .contains("busy"));
    assert_eq!(fs::read(fixture.0.path()).unwrap(), bytes);
    drop(other_process);
    assert_eq!(
        fixture
            .0
            .patch(Preferences {
                six_dof_speed: Some(2.),
                ..Default::default()
            })
            .unwrap()
            .six_dof_speed,
        Some(2.)
    );
}

#[test]
fn observer_refreshes_other_windows_and_external_writes_without_initializing_storage() {
    let fixture = Fixture::new();
    let mut observer = Observer::new(fixture.0.clone());
    let now = Instant::now();
    assert_eq!(
        observer.poll(now, false).unwrap().unwrap(),
        Preferences::default()
    );
    assert!(!fixture.0.path.parent().unwrap().exists());
    fixture
        .0
        .patch(Preferences {
            locale: Some(Locale::De),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        observer.poll(now, false).unwrap().unwrap().locale,
        Some(Locale::De)
    );
    fixture.write(br#"{"schema_version":1,"locale":"es"}"#);
    assert_eq!(
        observer.poll(now, true).unwrap().unwrap().locale,
        Some(Locale::Es)
    );
    fixture.write(br#"{"schema_version":1,"theme":"dark"}"#);
    assert_eq!(
        observer
            .poll(now + REFRESH_INTERVAL, false)
            .unwrap()
            .unwrap()
            .theme,
        Some(ThemePreference::Dark)
    );
    fixture.write("corrupt");
    assert!(observer.poll(now, true).unwrap().is_err());
    assert_eq!(fs::read(fixture.0.path()).unwrap(), b"corrupt");
}

#[test]
fn store_requires_an_absolute_config_folder_and_reports_unwritable_destinations() {
    assert!(Store::at(PathBuf::from("relative")).is_err());
    let fixture = Fixture::new();
    fixture.write(br#"{"schema_version":1,"locale":"en"}"#);
    let invalid = Store::at(fixture.0.path.clone()).unwrap();
    assert!(invalid
        .patch(Preferences {
            theme: Some(ThemePreference::Dark),
            ..Default::default()
        })
        .is_err());
    assert_eq!(fixture.0.read().unwrap().locale, Some(Locale::En));
}

#[test]
fn interface_size_round_trip_preserves_other_preferences_and_rejects_corrupt_sizes() {
    let fixture = Fixture::new();
    fixture.write(br#"{"schema_version":1,"theme":"dark","future":{"keep":true}}"#);
    let saved = fixture
        .0
        .patch(Preferences {
            ui_scale: Some(1.75),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(saved.theme, Some(ThemePreference::Dark));
    assert_eq!(saved.effective(Locale::En).ui_scale, 1.75);
    fixture
        .0
        .patch(Preferences {
            locale: Some(Locale::Es),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(fixture.0.read().unwrap().ui_scale, Some(1.75));
    let raw: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.0.path()).unwrap()).unwrap();
    assert_eq!(raw["future"]["keep"], true);
    fixture.write(br#"{"schema_version":1,"ui_scale":2}"#);
    assert!(fixture.0.read().is_err());
    assert!(fixture
        .0
        .patch(Preferences {
            ui_scale: Some(1.),
            ..Default::default()
        })
        .is_err());
}

#[test]
fn interface_size_snaps_read_and_written_preferences_to_visible_steps() {
    for (value, expected) in [
        (1.12, 1.1),
        (1.2, 1.25),
        (1.33, 1.25),
        (9., 1.75),
        (0.01, 0.9),
    ] {
        assert_eq!(snap_ui_scale(value), expected);
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(snap_ui_scale(value), DEFAULT_UI_SCALE);
    }
    for option in UI_SCALE_OPTIONS {
        assert_eq!(snap_ui_scale(option), option);
    }
    assert_eq!(step_ui_scale(1., true), 1.1);
    assert_eq!(step_ui_scale(1., false), 0.9);
    assert_eq!(step_ui_scale(0.9, false), 0.9);
    assert_eq!(step_ui_scale(1.75, true), 1.75);
    assert_eq!(step_ui_scale(1.33, true), 1.5);
    let fixture = Fixture::new();
    fixture.write(br#"{"schema_version":1,"ui_scale":1.33,"theme":"dark"}"#);
    let saved = fixture.0.read().unwrap();
    assert_eq!(saved.ui_scale, Some(1.25));
    assert_eq!(saved.effective(Locale::En).ui_scale, 1.25);
    let patched = fixture
        .0
        .patch(Preferences {
            ui_scale: Some(1.12),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(patched.ui_scale, Some(1.1));
    assert_eq!(patched.theme, Some(ThemePreference::Dark));
    assert_eq!(fixture.0.read().unwrap().ui_scale, Some(1.1));
}

#[test]
fn the_kernel_is_a_saved_choice_like_the_others_and_open_cascade_until_it_is_made() {
    let fixture = Fixture::new();
    // Nothing saved: Open CASCADE, and asking creates no files.
    assert_eq!(fixture.0.kernel(), KernelPreference::Occt);
    assert!(!fixture.0.path.parent().unwrap().exists());

    fixture.write(br#"{"schema_version":1,"theme":"dark","future":{"keep":true}}"#);
    let saved = fixture
        .0
        .patch(Preferences {
            kernel: Some(KernelPreference::Limo),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(saved.theme, Some(ThemePreference::Dark));
    assert_eq!(saved.effective(Locale::En).kernel, KernelPreference::Limo);
    assert_eq!(fixture.0.kernel(), KernelPreference::Limo);
    let raw: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.0.path()).unwrap()).unwrap();
    assert_eq!(raw["kernel"], "limo");
    assert_eq!(raw["future"]["keep"], true);

    // Another choice leaves it alone, and going back is a choice too.
    fixture
        .0
        .patch(Preferences {
            locale: Some(Locale::De),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(fixture.0.kernel(), KernelPreference::Limo);
    fixture
        .0
        .patch(Preferences {
            kernel: Some(KernelPreference::Occt),
            ..Default::default()
        })
        .unwrap();
    let raw: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.0.path()).unwrap()).unwrap();
    assert_eq!(raw["kernel"], "occt");
    assert_eq!(fixture.0.kernel(), KernelPreference::Occt);

    // A kernel this build does not know, and preferences it cannot read at
    // all, are not taken for the experimental kernel, and are not replaced.
    for text in [r#"{"schema_version":1,"kernel":"another"}"#, "not json"] {
        fixture.write(text);
        assert!(fixture.0.read().is_err());
        assert_eq!(fixture.0.kernel(), KernelPreference::Occt);
        assert!(fixture
            .0
            .patch(Preferences {
                kernel: Some(KernelPreference::Limo),
                ..Default::default()
            })
            .is_err());
        assert_eq!(fs::read(fixture.0.path()).unwrap(), text.as_bytes());
    }
}

#[test]
fn a_start_with_no_readable_preferences_stays_on_open_cascade() {
    // The saved choice of the experimental kernel is taken up by a start of
    // the application itself; a test must not make it for the whole process.
    assert_eq!(
        choose_saved_kernel(Err("no configuration folder".into())),
        KernelPreference::Occt
    );
    let fixture = Fixture::new();
    assert_eq!(
        choose_saved_kernel(Ok(fixture.0.clone())),
        KernelPreference::Occt
    );
    assert!(!fixture.0.path.parent().unwrap().exists());
    fixture.write("not json");
    assert_eq!(
        choose_saved_kernel(Ok(fixture.0.clone())),
        KernelPreference::Occt
    );
}
