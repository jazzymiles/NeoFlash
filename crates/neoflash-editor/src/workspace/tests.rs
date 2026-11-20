use super::*;
use std::collections::BTreeMap;

#[test]
fn video_edit_template_keeps_the_pre_beta_premiere_preference_key() {
    assert_eq!(WorkspaceTemplate::VideoEdit.label(), "Video edit");
    assert_eq!(
        serde_json::to_string(&WorkspaceTemplate::VideoEdit).unwrap(),
        "\"video_edit\""
    );
    assert_eq!(
        serde_json::from_str::<WorkspaceTemplate>("\"premiere\"").unwrap(),
        WorkspaceTemplate::VideoEdit
    );
    assert!(
        WorkspaceTemplate::ALL
            .iter()
            .all(|template| !template.label().contains("Premiere"))
    );
}

#[derive(Default)]
struct Storage {
    values: BTreeMap<String, String>,
    flushes: usize,
}
impl eframe::Storage for Storage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.values.insert(key.into(), value);
    }
    fn flush(&mut self) {
        self.flushes += 1;
    }
}
#[test]
fn workspace_roundtrip_writes_only_its_own_bounded_key() {
    let mut prefs = WorkspacePrefs {
        theme: ThemeMode::Dark,
        ..Default::default()
    };
    prefs.set_panel_visible(PanelId::Timeline, false);
    prefs.set_panel_visible(PanelId::Properties, false);
    prefs.show_panel(PanelId::Accessibility);
    prefs.sizes.library_width = 310.;
    let mut storage = Storage::default();
    storage.values.insert("unrelated".into(), "keep me".into());
    prefs.save(&mut storage);
    assert_eq!(WorkspacePrefs::load(Some(&storage)), prefs);
    assert_eq!(storage.values.len(), 2);
    assert_eq!(storage.values["unrelated"], "keep me");
    assert_eq!(storage.flushes, 0);
    assert!(storage.values[PREFS_KEY].len() < MAX_PREFS_BYTES);
    assert!(!storage.values[PREFS_KEY].contains("symbols"));
    assert!(!storage.values[PREFS_KEY].contains("keyframes"));
}
#[test]
fn every_theme_roundtrips_and_legacy_names_remain_compatible() {
    for theme in ThemeMode::ALL {
        let prefs = WorkspacePrefs {
            theme,
            ..Default::default()
        };
        let encoded = serde_json::to_string(&prefs).unwrap();
        assert_eq!(WorkspacePrefs::from_json(&encoded).theme, theme);
    }
    assert_eq!(
        WorkspacePrefs::from_json(r#"{"theme":"dark"}"#).theme,
        ThemeMode::Dark
    );
    assert_eq!(
        WorkspacePrefs::from_json(r#"{"theme":"light"}"#).theme,
        ThemeMode::Light
    );
    assert_eq!(
        WorkspacePrefs::from_json(r#"{"theme":"mid_neutral_gray"}"#).theme,
        ThemeMode::MidNeutralGray
    );
    assert_eq!(
        WorkspacePrefs::from_json(r#"{"theme":"dark_graphite"}"#).theme,
        ThemeMode::DarkGraphite
    );
}
#[test]
fn legacy_preferences_without_track2_panels_or_swatches_load_and_new_fields_roundtrip() {
    let legacy = r#"{"version":1,"theme":"dark","panels":{"timeline":true,"tools":true,"library":true,"properties":true,"actions":false,"accessibility":false,"output":false}}"#;
    let loaded = WorkspacePrefs::from_json(legacy);
    assert_eq!(loaded.theme, ThemeMode::Dark);
    assert!(!loaded.panels.history);
    assert!(!loaded.panels.swatches);
    assert!(!loaded.panels.info);
    assert!(!loaded.panels.project);
    assert!(loaded.recent_colors.is_empty());
    assert!(loaded.custom_swatches.is_empty());
    assert!(loaded.document_defaults.is_none());
    assert_eq!(loaded.version, VERSION);
    assert!(!loaded.timeline_prefs.span_selection);

    let mut before_brushes = loaded.clone();
    before_brushes.show_panel(PanelId::Filters);
    before_brushes.dock.remove(PanelId::Brushes, None);
    let migrated = WorkspacePrefs::from_json(&serde_json::to_string(&before_brushes).unwrap());
    assert!(migrated.dock.location(PanelId::Brushes).is_some());
    assert!(!migrated.is_panel_visible(PanelId::Brushes));
    assert!(
        migrated.is_panel_active(PanelId::Filters),
        "adding a hidden utility tab preserves the active saved tab"
    );

    let mut prefs = loaded;
    prefs.show_panel(PanelId::History);
    prefs.show_panel(PanelId::Swatches);
    prefs.show_panel(PanelId::Info);
    prefs.show_panel(PanelId::Project);
    prefs.recent_colors = vec![[91, 140, 255, 255], [255, 120, 64, 200]];
    prefs.custom_swatches = vec![[24, 32, 48, 255]];
    prefs.document_defaults = Some(DocumentDefaults {
        width: 1280,
        height: 720,
        fps: 30,
        background: [20, 24, 32, 255],
        units: crate::stage_view::RulerUnits::Pixels,
        script_language: neoflash_core::ScriptLanguage::TypeScript,
        also_compile_compatible_scripts: false,
    });
    let mut storage = Storage::default();
    prefs.save(&mut storage);
    assert_eq!(WorkspacePrefs::load(Some(&storage)), prefs);
}
#[test]
fn pre_v7_span_selection_migrates_off_but_v7_opt_in_persists() {
    let migrated =
        WorkspacePrefs::from_json(r#"{"version":1,"timeline_prefs":{"span_selection":true}}"#);
    assert_eq!(migrated.version, VERSION);
    assert!(!migrated.timeline_prefs.span_selection);
    assert!(!migrated.timeline_prefs.auto_keyframe);

    let v3_migrated =
        WorkspacePrefs::from_json(r#"{"version":3,"timeline_prefs":{"auto_keyframe":true}}"#);
    assert_eq!(v3_migrated.version, VERSION);
    assert!(!v3_migrated.timeline_prefs.auto_keyframe);

    let v5_migrated = WorkspacePrefs::from_json(r#"{"version":5}"#);
    assert_eq!(v5_migrated.version, VERSION);
    assert!(v5_migrated.dock.location(PanelId::Debugger).is_some());
    assert!(!v5_migrated.is_panel_visible(PanelId::Debugger));
    assert!(v5_migrated.is_panel_active(PanelId::Timeline));
    assert!(v5_migrated.dock.location(PanelId::Output).is_some());
    assert!(!v5_migrated.is_panel_active(PanelId::Debugger));

    let legacy_v6 =
        WorkspacePrefs::from_json(r#"{"version":6,"timeline_prefs":{"span_selection":true}}"#);
    assert!(!legacy_v6.timeline_prefs.span_selection);

    let mut opted_in = migrated;
    opted_in.timeline_prefs.span_selection = true;
    opted_in.timeline_prefs.auto_keyframe = true;
    let restored = WorkspacePrefs::from_json(&serde_json::to_string(&opted_in).unwrap());
    assert_eq!(restored.version, VERSION);
    assert!(restored.timeline_prefs.span_selection);
    assert!(restored.timeline_prefs.auto_keyframe);
}
#[test]
fn version_two_builtin_layout_refreshes_without_losing_preferences_or_named_presets() {
    let mut legacy = WorkspacePrefs {
        version: 2,
        ..WorkspacePrefs::default()
    };
    legacy.apply_template(WorkspaceTemplate::Developer);
    legacy.version = 2;
    legacy.panels.library = false;
    legacy.panels.properties = false;
    legacy.panels.color = true;
    legacy.dock.right.groups[0].active = PanelId::Color;
    legacy.detached.color.placement = PanelPlacement::Detached;
    legacy.active_named_preset = Some("My layout".into());
    legacy.unsaved_layout = true;
    legacy.theme = ThemeMode::Dark;
    legacy.timeline.compact_rows = true;
    legacy.timeline.loop_playback = false;
    legacy.code_editor.word_wrap = true;
    legacy.publish.html_wrapper = false;

    let mut named = WorkspacePrefs::default();
    named.apply_template(WorkspaceTemplate::Designer);
    let named = WorkspacePreset::capture(&named);
    legacy
        .named_presets
        .insert("My layout".into(), named.clone());

    let migrated = WorkspacePrefs::from_json(&serde_json::to_string(&legacy).unwrap());
    let (dock, panels, active_bottom) = WorkspaceTemplate::Developer.layout();
    assert_eq!(migrated.version, VERSION);
    assert_eq!(migrated.template, WorkspaceTemplate::Developer);
    assert_eq!(migrated.dock, dock);
    assert_eq!(migrated.panels, panels);
    assert_eq!(migrated.active_bottom, active_bottom);
    assert_eq!(migrated.detached, DetachedPanels::default());
    assert!(migrated.floating.is_empty());
    assert_eq!(migrated.active_named_preset, None);
    assert!(!migrated.unsaved_layout);

    assert_eq!(migrated.theme, ThemeMode::Dark);
    assert!(migrated.timeline.compact_rows);
    assert!(!migrated.timeline.loop_playback);
    assert!(migrated.code_editor.word_wrap);
    assert!(!migrated.publish.html_wrapper);
    assert_eq!(migrated.named_presets.get("My layout"), Some(&named));
}
#[test]
fn malformed_large_unknown_version_and_unknown_theme_preferences_fail_soft() {
    let default = WorkspacePrefs::default();
    assert_eq!(WorkspacePrefs::load(None), default);
    for input in [
        "",
        "{",
        "null",
        r#"{"version":8,"theme":"dark"}"#,
        r#"{"theme":"system"}"#,
        r#"{"sizes":{"timeline_height":null}}"#,
    ] {
        assert_eq!(WorkspacePrefs::from_json(input), default, "{input}");
    }
    assert_eq!(
        WorkspacePrefs::from_json(&" ".repeat(MAX_PREFS_BYTES + 1)),
        default
    );
    let partial = WorkspacePrefs::from_json(
        r#"{"theme":"dark","panels":{"library":false},"unknown":"ignored"}"#,
    );
    assert_eq!(partial.theme, ThemeMode::Dark);
    assert!(!partial.panels.library);
    assert!(partial.panels.tools);
    assert_eq!(partial.sizes, PanelSizes::default());
}
#[test]
fn hidden_tabs_reopen_and_active_tab_falls_back_without_losing_dock_state() {
    let mut prefs = WorkspacePrefs::default();
    prefs.show_panel(PanelId::Actions);
    assert_eq!(prefs.active_bottom, BottomTab::Actions);
    prefs.set_panel_visible(PanelId::Actions, false);
    assert_eq!(prefs.active_bottom, BottomTab::Properties);
    for panel in PanelId::ALL {
        prefs.set_panel_visible(panel, false);
    }
    assert!(!prefs.bottom_visible());
    assert!(PanelId::ALL.into_iter().all(|p| !prefs.is_panel_visible(p)));
    assert!(prefs.show_panel(PanelId::Actions));
    assert!(prefs.bottom_visible());
    assert_eq!(prefs.active_bottom, BottomTab::Actions);
    assert!(!prefs.panels.properties);
    assert!(!prefs.show_panel(PanelId::Actions));
    for panel in PanelId::ALL {
        prefs.show_panel(panel);
        assert!(prefs.is_panel_visible(panel));
    }
}
#[test]
fn every_template_is_a_valid_layout_that_keeps_every_panel_reachable() {
    for template in WorkspaceTemplate::ALL {
        let mut prefs = WorkspacePrefs {
            theme: ThemeMode::Light,
            ..Default::default()
        };
        prefs.set_placement(PanelId::Library, PanelPlacement::Detached);
        prefs.apply_template(template);
        assert_eq!(prefs.template, template);
        assert_eq!(
            prefs.theme,
            ThemeMode::Light,
            "{template:?} keeps the theme"
        );
        assert!(
            prefs.floating.is_empty(),
            "{template:?} closes floating windows"
        );
        let mut seen = std::collections::HashSet::new();
        for edge in [
            DockEdge::Top,
            DockEdge::Bottom,
            DockEdge::Left,
            DockEdge::Right,
        ] {
            for group in &prefs.dock.area(edge).groups {
                assert!(group.tabs.contains(&group.active));
                for panel in &group.tabs {
                    assert!(seen.insert(*panel), "{template:?}: {panel:?} docked twice");
                }
            }
        }
        for panel in PanelId::ALL {
            assert!(
                prefs.is_panel_docked(panel),
                "{template:?}: {panel:?} has a place"
            );
        }
        let mut storage = Storage::default();
        prefs.save(&mut storage);
        assert_eq!(
            WorkspacePrefs::load(Some(&storage)),
            prefs,
            "{template:?} round-trips"
        );
    }
}
#[test]
fn restore_layout_preserves_theme_and_resets_visibility_sizes_and_active_tab() {
    let mut prefs = WorkspacePrefs {
        theme: ThemeMode::Dark,
        ..Default::default()
    };
    for panel in PanelId::ALL {
        prefs.set_panel_visible(panel, false);
    }
    prefs.sizes = PanelSizes {
        timeline_height: 380.,
        bottom_height: 420.,
        library_width: 360.,
        layer_column_width: 420.,
    };
    prefs.apply_template(WorkspaceTemplate::Classic);
    assert_eq!(
        prefs,
        WorkspacePrefs {
            theme: ThemeMode::Dark,
            ..Default::default()
        }
    );
}
#[test]
fn dimensions_are_normalized_before_persistence_without_mutating_live_preferences() {
    let prefs = WorkspacePrefs {
        sizes: PanelSizes {
            timeline_height: f32::NAN,
            bottom_height: f32::INFINITY,
            library_width: -100.,
            layer_column_width: f32::NAN,
        },
        ..Default::default()
    };
    let mut storage = Storage::default();
    prefs.save(&mut storage);
    let loaded = WorkspacePrefs::load(Some(&storage));
    assert_eq!(
        loaded.sizes,
        PanelSizes {
            timeline_height: crate::theme::timeline_h(),
            bottom_height: 190.,
            library_width: 248.,
            layer_column_width: neoflash_ui::metrics::LAYER_COLUMN_MODE_SOLO,
        }
    );
    assert!(prefs.sizes.timeline_height.is_nan());
    let large = WorkspacePrefs::from_json(
        r#"{"sizes":{"timeline_height":999999,"bottom_height":-1,"library_width":999999}}"#,
    );
    assert_eq!(
        large.sizes,
        PanelSizes {
            timeline_height: 380.,
            bottom_height: 145.,
            library_width: 360.,
            layer_column_width: neoflash_ui::metrics::LAYER_COLUMN_MODE_SOLO,
        }
    );
}
#[test]
fn restoring_preferences_cannot_select_a_hidden_tab_unless_all_tabs_are_hidden() {
    let prefs = WorkspacePrefs::from_json(
        r#"{"active_bottom":"Output","panels":{"output":false,"properties":false}}"#,
    );
    assert_eq!(prefs.active_bottom, BottomTab::Actions);
    let prefs = WorkspacePrefs::from_json(
        r#"{"active_bottom":"Output","panels":{"timeline":false,"properties":false,"actions":false,"accessibility":false,"output":false}}"#,
    );
    assert_eq!(prefs.active_bottom, BottomTab::Output);
    assert!(!prefs.bottom_visible());
}
fn screen(origin: [f32; 2], size: [f32; 2]) -> ScreenRect {
    ScreenRect { origin, size }
}
fn window(position: [f32; 2], size: [f32; 2]) -> WindowGeometry {
    WindowGeometry {
        outer_position: position,
        inner_size: size,
    }
}
#[test]
fn detached_actions_is_one_panel_and_reopens_in_the_same_placement() {
    let mut prefs = WorkspacePrefs::default();
    assert!(prefs.set_placement(PanelId::Actions, PanelPlacement::Detached));
    assert!(prefs.panels.actions);
    assert_eq!(prefs.active_bottom, BottomTab::Properties);
    assert!(!prefs.is_panel_docked(PanelId::Actions));
    assert!(prefs.set_placement(PanelId::Library, PanelPlacement::Detached));
    assert!(!prefs.is_panel_docked(PanelId::Library));
    // NF2: Timeline shares the bottom dock with Actions, Output and Accessibility.
    for panel in [
        PanelId::Timeline,
        PanelId::Properties,
        PanelId::Accessibility,
        PanelId::Output,
    ] {
        prefs.set_panel_visible(panel, false);
    }
    assert!(!prefs.bottom_visible());
    let geometry = window([-600., 70.], [640., 430.]);
    assert!(prefs.remember_geometry(PanelId::Actions, geometry));
    prefs.set_panel_visible(PanelId::Actions, false);
    assert!(prefs.show_panel(PanelId::Actions));
    assert_eq!(prefs.placement(PanelId::Actions), PanelPlacement::Detached);
    assert_eq!(prefs.detached_geometry(PanelId::Actions), Some(geometry));
    assert!(!prefs.bottom_visible());
    prefs.set_placement(PanelId::Actions, PanelPlacement::Docked);
    assert_eq!(prefs.active_bottom, BottomTab::Actions);
    assert!(prefs.bottom_visible());
}
#[test]
fn detached_preferences_roundtrip_migrate_and_restore_without_changing_theme() {
    let old = WorkspacePrefs::from_json(r#"{"version":1,"theme":"dark"}"#);
    assert_eq!(old.detached, DetachedPanels::default());
    for panel in [PanelId::Align, PanelId::Transform, PanelId::Color] {
        assert!(!old.is_panel_visible(panel));
        assert!(old.is_panel_docked(panel));
    }
    let mut prefs = old;
    // A real workspace arrangement: merge two tabs, reorder them, split
    // Properties back out, and move Timeline to a different edge.
    let library = prefs.dock.location(PanelId::Library).unwrap();
    prefs.dock_panel(
        PanelId::Properties,
        DockDestination::Tabs(DockLocation {
            index: 1,
            ..library
        }),
    );
    prefs.dock_panel(
        PanelId::Properties,
        DockDestination::Tabs(DockLocation {
            index: 0,
            ..library
        }),
    );
    assert_eq!(
        prefs
            .dock
            .right
            .groups
            .iter()
            .find(|g| g.tabs.contains(&PanelId::Library))
            .unwrap()
            .tabs,
        [PanelId::Properties, PanelId::Library]
    );
    prefs.dock_panel(
        PanelId::Properties,
        DockDestination::Split {
            edge: DockEdge::Right,
            group: library.group,
            before: true,
        },
    );
    assert_eq!(
        prefs
            .dock
            .right
            .groups
            .iter()
            .filter(|group| { group.tabs.iter().any(|p| prefs.is_panel_visible(*p)) })
            .count(),
        2
    );
    prefs.dock.right.groups[0].weight = 0.35;
    prefs.dock.right.groups[1].weight = 0.65;
    prefs.dock_panel(PanelId::Timeline, DockDestination::Edge(DockEdge::Bottom));
    let timeline_location = prefs.dock.location(PanelId::Timeline).unwrap();
    prefs.set_placement(PanelId::Timeline, PanelPlacement::Detached);
    prefs.set_placement(PanelId::Timeline, PanelPlacement::Docked);
    assert_eq!(
        prefs.dock.location(PanelId::Timeline).unwrap().edge,
        timeline_location.edge
    );
    let mut arranged_storage = Storage::default();
    prefs.save(&mut arranged_storage);
    assert_eq!(WorkspacePrefs::load(Some(&arranged_storage)), prefs);
    let mut legacy = serde_json::to_value(&prefs).unwrap();
    for key in ["align", "transform", "color"] {
        legacy["panels"].as_object_mut().unwrap().remove(key);
        legacy["detached"].as_object_mut().unwrap().remove(key);
    }
    for edge in ["top", "bottom", "left", "right"] {
        for group in legacy["dock"][edge]["groups"].as_array_mut().unwrap() {
            group["tabs"]
                .as_array_mut()
                .unwrap()
                .retain(|tab| !matches!(tab.as_str(), Some("align" | "transform" | "color")));
        }
    }
    let migrated = WorkspacePrefs::from_json(&legacy.to_string());
    for panel in [PanelId::Align, PanelId::Transform, PanelId::Color] {
        assert!(!migrated.is_panel_visible(panel));
        assert_eq!(
            migrated.dock.location(panel).unwrap().group,
            migrated.dock.location(PanelId::Properties).unwrap().group
        );
    }
    for panel in PanelId::DETACHABLE {
        prefs.set_placement(panel, PanelPlacement::Detached);
        prefs.remember_geometry(panel, window([1800., -200.], default_window_size(panel)));
    }
    // Native-window groups survive reordering, two-axis splitting, hiding,
    // and moving the panel that originally supplied the window identity.
    prefs.dock_panel(
        PanelId::Properties,
        DockDestination::FloatingTabs {
            window: PanelId::Library,
            group: 1,
            index: 1,
        },
    );
    prefs.dock_panel(
        PanelId::Properties,
        DockDestination::FloatingTabs {
            window: PanelId::Library,
            group: 1,
            index: 0,
        },
    );
    prefs.dock_panel(
        PanelId::Actions,
        DockDestination::FloatingSplit {
            window: PanelId::Library,
            group: None,
            edge: DockEdge::Right,
        },
    );
    prefs.dock_panel(
        PanelId::Tools,
        DockDestination::FloatingSplit {
            window: PanelId::Library,
            group: None,
            edge: DockEdge::Top,
        },
    );
    prefs.dock_panel(PanelId::Library, DockDestination::Edge(DockEdge::Left));
    assert_eq!(
        prefs.panel_surface(PanelId::Properties),
        DockSurface::Floating(PanelId::Library)
    );
    prefs.dock_panel(
        PanelId::Output,
        DockDestination::FloatingTabs {
            window: PanelId::Library,
            group: 1,
            index: 1,
        },
    );
    prefs.set_panel_visible(PanelId::Properties, false);
    let floating_window = prefs
        .floating
        .iter_mut()
        .find(|window| window.id == PanelId::Library)
        .unwrap();
    floating_window.set_ratio(&[], 0.35);
    assert_eq!(
        floating_window.group(1).unwrap().tabs,
        [PanelId::Properties, PanelId::Output]
    );
    let all_panels: Vec<_> = prefs
        .floating
        .iter()
        .flat_map(FloatingDock::panels)
        .collect();
    assert_eq!(
        all_panels
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        all_panels.len()
    );
    let mut storage = Storage::default();
    prefs.save(&mut storage);
    assert_eq!(WorkspacePrefs::load(Some(&storage)), prefs);
    assert!(storage.values[PREFS_KEY].len() < MAX_PREFS_BYTES);
    assert!(
        PanelId::DETACHABLE
            .into_iter()
            .all(|p| p.viewport_key().is_some())
    );
    assert_ne!(
        PanelId::Tools.viewport_key(),
        PanelId::Timeline.viewport_key()
    );
    // A header moves its entire tab group, preserving hidden utility tabs,
    // while all four root/native edges remain valid destinations.
    let tabs = [
        PanelId::Properties,
        PanelId::Align,
        PanelId::Transform,
        PanelId::Color,
    ];
    for edge in DockEdge::ALL {
        let mut grouped = WorkspacePrefs::default();
        grouped.dock_group(&tabs, PanelId::Properties, DockDestination::Edge(edge));
        let location = grouped.dock.location(PanelId::Properties).unwrap();
        assert_eq!(location.edge, edge);
        assert!(
            tabs.iter()
                .all(|p| grouped.dock.location(*p).unwrap().group == location.group)
        );
        let floating = grouped.float_group(
            &tabs,
            PanelId::Properties,
            window([300., 200.], [640., 480.]),
        );
        assert_eq!(
            grouped
                .floating_window(PanelId::Properties)
                .unwrap()
                .panels(),
            tabs
        );
        assert_eq!(
            grouped.float_group(
                &tabs,
                PanelId::Properties,
                window([320., 200.], [640., 480.])
            ),
            floating
        );
        let other = grouped.float_panel(PanelId::Library, None);
        grouped.dock_group(
            &tabs,
            PanelId::Properties,
            DockDestination::FloatingSplit {
                window: other,
                group: None,
                edge,
            },
        );
        assert_eq!(
            grouped.panel_surface(PanelId::Properties),
            DockSurface::Floating(other)
        );
        let tree = grouped
            .floating_window(PanelId::Properties)
            .unwrap()
            .root
            .as_ref()
            .unwrap();
        let group = tree.location(PanelId::Properties).unwrap().0;
        assert_eq!(tree.group(group).unwrap().tabs, tabs);
        assert_eq!(tree.group(group).unwrap().active, PanelId::Properties);
        for panel in &tabs[1..] {
            assert!(!grouped.is_panel_visible(*panel));
        }
        grouped.dock_group(&tabs, PanelId::Properties, DockDestination::Edge(edge));
        let encoded = serde_json::to_string(&grouped).unwrap();
        assert_eq!(WorkspacePrefs::from_json(&encoded), grouped);
    }
    prefs.apply_template(WorkspaceTemplate::Classic);
    assert_eq!(prefs.theme, ThemeMode::Dark);
    assert_eq!(prefs.detached, DetachedPanels::default());
}
#[test]
fn invalid_geometry_cannot_overwrite_last_good_snapshot() {
    let mut prefs = WorkspacePrefs::default();
    let good = window([70., 80.], [680., 440.]);
    prefs.remember_geometry(PanelId::Actions, good);
    for bad in [
        window([f32::NAN, 0.], [680., 440.]),
        window([0., 0.], [0., 440.]),
        window([0., 0.], [f32::INFINITY, 440.]),
    ] {
        assert!(!prefs.remember_geometry(PanelId::Actions, bad));
        assert_eq!(prefs.detached_geometry(PanelId::Actions), Some(good));
    }
    assert!(prefs.remember_geometry(PanelId::Library, good));
    let parsed = WorkspacePrefs::from_json(
        r#"{"theme":"dark","detached":{"actions":{"placement":"detached","geometry":{"outer_position":[1e30,0],"inner_size":[600,400]}}}}"#,
    );
    assert_eq!(parsed.theme, ThemeMode::Dark);
    assert_eq!(parsed.placement(PanelId::Actions), PanelPlacement::Detached);
    assert_eq!(parsed.detached_geometry(PanelId::Actions), None);
}
#[test]
fn recovery_preserves_connected_secondary_and_partial_windows() {
    let main = screen([0., 24.], [1440., 826.]);
    let left = screen([-1920., -180.], [1920., 1080.]);
    for geometry in [
        window([-1800., -100.], [680., 440.]),
        window([1400., 60.], [680., 440.]),
        window([-500., 60.], [680., 440.]),
    ] {
        let recovered = recover_geometry(
            PanelId::Actions,
            Some(geometry),
            &[main, left, screen([1440., 0.], [1920., 1080.])],
            main,
        );
        assert_eq!(recovered, geometry);
    }
}
#[test]
fn recovery_requires_a_reachable_titlebar_and_fits_removed_monitor_geometry() {
    let main = screen([0., 24.], [1440., 826.]);
    for geometry in [
        window([-1800., -100.], [680., 440.]),
        window([100., -300.], [680., 440.]),
        window([1430., 50.], [680., 440.]),
    ] {
        let recovered = recover_geometry(PanelId::Actions, Some(geometry), &[main], main);
        assert!(recovered.outer_position[0] >= 0.);
        assert!(recovered.outer_position[1] >= 24.);
        assert!(recovered.outer_position[0] + recovered.inner_size[0] + 16. <= 1440.);
        assert!(recovered.outer_position[1] + recovered.inner_size[1] + 40. <= 850.);
    }
    // A sliver of content below the menu bar cannot make its titlebar usable.
    let titlebar_hidden = window([100., 0.], [680., 440.]);
    assert_eq!(
        recover_geometry(PanelId::Actions, Some(titlebar_hidden), &[main], main).outer_position[1],
        24.
    );
}
#[test]
fn recovery_bounds_huge_sizes_and_invalid_screen_input() {
    let small = screen([20., 30.], [500., 400.]);
    let recovered = recover_geometry(
        PanelId::Actions,
        Some(window([50., 50.], [1e20, 1e20])),
        &[small],
        small,
    );
    assert_eq!(recovered, window([20., 30.], [484., 360.]));
    let invalid = screen([f32::NAN, 0.], [0., f32::INFINITY]);
    let recovered = recover_geometry(PanelId::Tools, None, &[invalid], invalid);
    assert!(recovered.outer_position.into_iter().all(f32::is_finite));
    assert!(
        recovered
            .inner_size
            .into_iter()
            .all(|v| v.is_finite() && v > 0.)
    );
    let tiny = screen([999_999., 0.], [120.1, 120.1]);
    let recovered = recover_geometry(
        PanelId::Actions,
        Some(window([999_999., 0.], [680., 440.])),
        &[tiny],
        tiny,
    );
    assert!(recovered.inner_size[0] < 120.1 && recovered.inner_size[1] < 120.1);
}

#[test]
fn shortcut_prefs_round_trips_manages_sets_and_sanitizes_invalid_chords() {
    use crate::commands::{CommandId, Shortcut, effective_shortcuts};
    use eframe::egui::Key;

    let mut prefs = WorkspacePrefs::default();
    prefs
        .shortcuts
        .set_shortcuts(CommandId::InsertKeyframe, vec![Shortcut::alt(Key::F6)]);
    let copy_name = prefs.shortcuts.duplicate_active_set();
    assert_eq!(copy_name, "NeoFlash default copy");
    assert_eq!(prefs.shortcuts.active_set_label(), "NeoFlash default copy");

    let copy2 = prefs.shortcuts.duplicate_active_set();
    assert_eq!(copy2, "NeoFlash default copy copy");
    assert!(prefs.shortcuts.delete_active_set());
    assert!(prefs.shortcuts.is_default_set());
    assert!(!prefs.shortcuts.delete_active_set());
    assert_eq!(
        effective_shortcuts(&prefs.shortcuts, CommandId::InsertKeyframe),
        vec![Shortcut::none(Key::F6)]
    );

    prefs.shortcuts.select_set("NeoFlash default copy");
    assert_eq!(
        effective_shortcuts(&prefs.shortcuts, CommandId::InsertKeyframe),
        vec![Shortcut::alt(Key::F6)]
    );

    let json = serde_json::to_string(&prefs).unwrap();
    let restored = WorkspacePrefs::from_json(&json);
    assert_eq!(restored.shortcuts, prefs.shortcuts);

    let dirty = WorkspacePrefs::from_json(
        r#"{
                "shortcuts": {
                    "set": "  Custom 🚀  ",
                    "overrides": {
                        "insert.keyframe": ["alt+f6", "not-a-key", ""],
                        "tool.polystar": [],
                        "unknown.command": ["F1"]
                    },
                    "sets": {
                        "": {"insert.frame": ["F5"]},
                        "NeoFlash default": {"insert.frame": ["F9"]}
                    }
                }
            }"#,
    );
    assert_eq!(dirty.shortcuts.set, "Custom 🚀");
    assert_eq!(
        dirty.shortcuts.overrides.get("insert.keyframe"),
        Some(&vec!["⌥F6".to_owned()])
    );
    // Empty vec override (explicitly unbound) survives sanitization; unknown IDs are dropped.
    assert_eq!(
        dirty.shortcuts.overrides.get("tool.polystar"),
        Some(&vec![])
    );
    assert!(!dirty.shortcuts.overrides.contains_key("unknown.command"));
    assert!(!dirty.shortcuts.sets.contains_key(""));
    assert!(!dirty.shortcuts.sets.contains_key("NeoFlash default"));
    assert!(dirty.shortcuts.sets.contains_key("Custom 🚀"));
}

/// T-389: panel size tables come from density metrics. Inspector-family panels share the
/// `inspector()` minimum, wider panels scale with it, and every default fits its limits.
#[test]
fn window_size_tables_follow_density_metrics() {
    use neoflash_ui::metrics::{self, Density};
    let saved = metrics::density();
    for density in Density::ALL {
        metrics::set_density(density);
        let inspector = metrics::inspector();
        assert_eq!(
            minimum_window_size(PanelId::Tools)[0],
            metrics::tools_rail()
        );
        assert_eq!(
            default_window_size(PanelId::Tools)[0],
            metrics::tools_two_columns()
        );
        for panel in PanelId::DETACHABLE {
            let (min, max, default) = (
                minimum_window_size(panel),
                maximum_window_size(panel),
                default_window_size(panel),
            );
            for axis in 0..2 {
                assert!(min[axis] <= default[axis], "{panel:?} in {density:?}");
                assert!(default[axis] <= max[axis], "{panel:?} in {density:?}");
            }
            if panel != PanelId::Tools {
                assert!(min[0] >= inspector, "{panel:?} in {density:?}");
                assert_eq!(min[0] % 4., 0., "{panel:?} in {density:?}");
            }
        }
        for panel in [PanelId::Properties, PanelId::Transform, PanelId::Library] {
            assert_eq!(minimum_window_size(panel)[0], inspector);
            assert_eq!(default_window_size(panel)[0], inspector);
        }
        for (panel, old) in [
            (PanelId::Timeline, 320.),
            (PanelId::Ease, 320.),
            (PanelId::CompilerErrors, 360.),
            (PanelId::Debugger, 620.),
            (PanelId::MotionEditor, 640.),
        ] {
            let scaled = old * inspector / 320.;
            assert!(
                (minimum_window_size(panel)[0] - scaled).abs() <= 2.,
                "{panel:?} in {density:?}"
            );
        }
    }
    metrics::set_density(saved);
}

/// T-389: saved workspaces keep loading. A dock width remembered by an older build is kept,
/// and saved window geometry is checked against the saved density profile, not against the
/// profile of the thread that loads it.
#[test]
fn saved_sizes_survive_the_density_based_size_tables() {
    use neoflash_ui::metrics::{self, Density};
    let saved = metrics::density();
    metrics::set_density(Density::Comfortable);

    // The former 320 pt inspector (+ 6 pt strip) and a hand-sized dock both stay as saved.
    for width in [326., 412.] {
        let mut prefs = WorkspacePrefs::default();
        prefs.dock.right.size = width;
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(WorkspacePrefs::from_json(&json).dock.right.size, width);
    }

    // A Classic Flash window at that profile's minimum is not widened by a Normal-density loader.
    let classic = density_metric("inspector", Density::ClassicFlash);
    assert!(classic < metrics::inspector());
    let mut prefs = WorkspacePrefs::default();
    prefs.set_density(DensityPref::ClassicFlash);
    assert!(prefs.set_placement(PanelId::Transform, PanelPlacement::Detached));
    metrics::set_density(Density::ClassicFlash);
    prefs.remember_geometry(PanelId::Transform, window([200., 120.], [classic, 300.]));
    metrics::set_density(Density::Comfortable);
    let json = serde_json::to_string(&prefs).unwrap();
    let loaded = WorkspacePrefs::from_json(&json);
    assert_eq!(metrics::density(), Density::Comfortable);
    assert_eq!(
        loaded
            .detached_geometry(PanelId::Transform)
            .unwrap()
            .inner_size,
        [classic, 300.]
    );

    // Switching profile moves default-width docks to the new default and keeps chosen widths.
    let mut prefs = WorkspacePrefs::default();
    assert_eq!(prefs.dock.right.size, metrics::inspector() + 6.);
    prefs.set_density(DensityPref::Compact);
    assert_eq!(
        prefs.dock.right.size,
        density_metric("inspector", Density::Compact) + 6.
    );
    assert_eq!(
        prefs.dock.left.size,
        density_metric("tools_two_columns", Density::Compact) + 6.
    );
    prefs.dock.right.size = 412.;
    prefs.set_density(DensityPref::ClassicFlash);
    assert_eq!(prefs.dock.right.size, 412.);
    metrics::set_density(saved);
}
