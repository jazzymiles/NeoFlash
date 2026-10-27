use neoflash_core::{PlaybackCommand, Player, ScriptLanguage, evaluate};
use neoflash_import::{Severity, import_project, inspect};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Source(PathBuf);
impl Source {
    fn new(fixture: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-xfl-publish-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("LIBRARY")).unwrap();
        let original = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        for name in ["DOMDocument.xml", "PublishSettings.xml"] {
            fs::copy(original.join(name), dir.join(name)).unwrap();
        }
        for entry in fs::read_dir(original.join("LIBRARY")).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), dir.join("LIBRARY").join(entry.file_name())).unwrap();
        }
        Self(dir)
    }
    fn write(&self, name: &str, text: &str) {
        fs::write(self.0.join(name), text).unwrap();
    }
    fn replace(&self, name: &str, before: &str, after: &str) {
        let text = fs::read_to_string(self.0.join(name)).unwrap();
        assert!(text.contains(before), "missing {before}");
        self.write(name, &text.replacen(before, after, 1));
    }
    fn policy(&self, value: &str) {
        self.write(
            "PublishSettings.xml",
            &format!(
                "<flash_profiles>{}</flash_profiles>",
                profile("true", "2", value)
            ),
        );
    }
    fn blocked(&self, code: &str) {
        let report = inspect(&self.0).unwrap();
        assert!(!report.can_import, "accepted unsupported source");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error && d.code == code),
            "expected {code}: {:?}",
            report.diagnostics
        );
        assert!(import_project(&self.0).is_err());
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn profile(current: &str, language: &str, policy: &str) -> String {
    format!(
        "<flash_profile current=\"{current}\"><PublishFlashProperties><ActionScriptVersion>{language}</ActionScriptVersion>{policy}</PublishFlashProperties></flash_profile>"
    )
}

#[test]
fn root_and_nested_hidden_layers_follow_publish_policy_and_preserve_all_artwork() {
    for (value, includes) in [
        ("", true),
        ("<InvisibleLayer>1</InvisibleLayer>", true),
        ("<InvisibleLayer>0</InvisibleLayer>", false),
    ] {
        let source = Source::new("hidden-layers");
        source.policy(value);
        let report = inspect(&source.0).unwrap();
        assert!(report.can_import, "{:?}", report.diagnostics);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|d| d.code == "hidden_layer_publish_policy")
                .count(),
            2
        );
        let project = import_project(&source.0).unwrap();
        assert_eq!(
            project
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.layers)
                .flat_map(|l| &l.keyframes)
                .map(|k| k.elements.len())
                .sum::<usize>(),
            5
        );
        for symbol in &project.symbols {
            let hidden = symbol
                .timeline
                .layers
                .iter()
                .find(|l| l.name.starts_with("Hidden"))
                .unwrap();
            assert_eq!(hidden.visible, includes);
        }
        let authored = evaluate(&project, &project.root, 0);
        let mut player = Player::new(&project);
        assert_eq!(authored.len(), if includes { 4 } else { 2 });
        assert_eq!(player.evaluate(&project).len(), authored.len());
        assert_eq!(authored.iter().any(|e| e.name == "rootRed"), includes);
        assert_eq!(authored.iter().any(|e| e.name == "nestedBlue"), includes);
        assert_eq!(player.drain_scripts().len(), 2); // Only the two visible-layer scripts exist.
    }
}

#[test]
fn included_hidden_layer_source_remains_executable_but_excluded_source_is_blocked() {
    for path in ["DOMDocument.xml", "LIBRARY/Nested.xml"] {
        let source = Source::new("hidden-layers");
        source.replace(path, "<DOMFrame index=\"0\" duration=\"12\"><elements>", "<DOMFrame index=\"0\" duration=\"12\"><Actionscript><script>trace(\"hidden-source\");</script></Actionscript><elements>");
        let project = import_project(&source.0).unwrap();
        let mut player = Player::new(&project);
        let scripts = player.drain_scripts();
        assert_eq!(scripts.len(), 3);
        assert!(
            scripts
                .iter()
                .any(|s| s.script.source == "trace(\"hidden-source\");")
        );
        source.policy("<InvisibleLayer>0</InvisibleLayer>");
        source.blocked("excluded_layer_script");
    }
}

#[test]
fn excluded_hidden_labels_and_unestablished_duration_block_in_root_and_nested_timelines() {
    for path in ["DOMDocument.xml", "LIBRARY/Nested.xml"] {
        let source = Source::new("hidden-layers");
        source.policy("<InvisibleLayer>0</InvisibleLayer>");
        source.replace(path, "<DOMFrame index=\"0\" duration=\"12\"><elements>", "<DOMFrame index=\"0\" duration=\"12\" name=\"hidden-label\" labelType=\"name\"><elements>");
        source.blocked("excluded_layer_label");
        source.replace(path, " name=\"hidden-label\" labelType=\"name\"", "");
        source.replace(
            path,
            "<DOMFrame index=\"0\" duration=\"12\"><elements>",
            "<DOMFrame index=\"0\" duration=\"14\"><elements>",
        );
        source.blocked("excluded_layer_duration");
    }
}

#[test]
fn malformed_or_ambiguous_hidden_layer_settings_are_never_defaulted_silently() {
    for policy in [
        "<InvisibleLayer/>",
        "<InvisibleLayer>2</InvisibleLayer>",
        "<InvisibleLayer>FALSE</InvisibleLayer>",
        "<InvisibleLayer>true</InvisibleLayer>",
        "<InvisibleLayer>false</InvisibleLayer>",
        "<InvisibleLayer>0</InvisibleLayer><InvisibleLayer>0</InvisibleLayer>",
        "<InvisibleLayer>0</InvisibleLayer><InvisibleLayer>1</InvisibleLayer>",
        "<InvisibleLayer>0<!-- split -->1</InvisibleLayer>",
        "<InvisibleLayer><value>0</value></InvisibleLayer>",
        "<InvisibleLayer enabled=\"true\">0</InvisibleLayer>",
        "<InvisibleLayer xmlns=\"urn:unknown\">0</InvisibleLayer>",
        "<Unknown><InvisibleLayer>0</InvisibleLayer></Unknown>",
        "<IncludeHiddenLayers>0</IncludeHiddenLayers>",
    ] {
        let source = Source::new("hidden-layers");
        source.policy(policy);
        source.blocked("publish_layer_policy");
    }
}

#[test]
fn current_publish_profile_selects_language_and_visibility_together() {
    let source = Source::new("hidden-layers");
    for active_first in [false, true] {
        let current = profile("true", "2", "<InvisibleLayer>1</InvisibleLayer>");
        let inactive = profile(
            "false",
            "3",
            "<InvisibleLayer>0</InvisibleLayer><DocumentClass>Unused</DocumentClass>",
        );
        let joined = if active_first {
            format!("{current}{inactive}")
        } else {
            format!("{inactive}{current}")
        };
        source.write(
            "PublishSettings.xml",
            &format!("<flash_profiles>{joined}</flash_profiles>"),
        );
        let project = import_project(&source.0).unwrap();
        assert_eq!(evaluate(&project, &project.root, 0).len(), 4);
        assert!(
            project
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.scripts)
                .all(|s| s.script.language == ScriptLanguage::ActionScript2)
        );
    }
}

#[test]
fn invalid_profile_selection_and_misplaced_policy_block_import() {
    let good = profile("true", "2", "<InvisibleLayer>1</InvisibleLayer>");
    for body in [
        format!("{good}{good}"), profile("false", "2", ""), profile("maybe", "2", ""),
        format!("<Unknown>{good}</Unknown>"),
        good.replace("<PublishFlashProperties>", "<PublishFlashProperties xmlns=\"urn:unknown\">"),
        good.replace("</flash_profile>", "<PublishFlashProperties><ActionScriptVersion>2</ActionScriptVersion></PublishFlashProperties></flash_profile>"),
    ] {
        let source = Source::new("hidden-layers");
        source.write("PublishSettings.xml", &format!("<flash_profiles>{body}</flash_profiles>"));
        source.blocked("publish_profile");
    }
    let source = Source::new("hidden-layers");
    source.write(
        "PublishSettings.xml",
        &format!("<flash_profiles>{good}<InvisibleLayer>0</InvisibleLayer></flash_profiles>"),
    );
    source.blocked("publish_profile");
    source.write(
        "PublishSettings.xml",
        &format!(
            "<flash_profiles>{}</flash_profiles>",
            good.replace(
                "</flash_profile>",
                "<InvisibleLayer>0</InvisibleLayer></flash_profile>"
            )
        ),
    );
    source.blocked("publish_layer_policy");
}

#[test]
fn two_held_instances_keep_distinct_clocks_and_lifetimes() {
    let source = Source::new("instance-gap");
    let project = import_project(&source.0).unwrap();
    let mut player = Player::new(&project);
    let a = player.child_path(&project, &[], "a").unwrap();
    let b = player.child_path(&project, &[], "b").unwrap();
    let life_a = player.lifetime_id(&a).unwrap();
    let life_b = player.lifetime_id(&b).unwrap();
    assert_ne!(a, b);
    assert_ne!(life_a, life_b);
    assert!(player.control(&project, &b, PlaybackCommand::Stop));
    player.tick(&project);
    assert_eq!(player.clip_frame(&a), Some(1));
    assert_eq!(player.clip_frame(&b), Some(0));
    assert_eq!(player.lifetime_id(&a), Some(life_a));
    assert_eq!(player.lifetime_id(&b), Some(life_b));
}

#[test]
fn direct_goto_across_explicit_or_implicit_blank_gap_creates_fresh_reordered_placements() {
    for explicit in [false, true] {
        let source = Source::new("instance-gap");
        if !explicit {
            source.replace(
                "DOMDocument.xml",
                "<DOMFrame index=\"2\"><elements/></DOMFrame>",
                "",
            );
        }
        let project = import_project(&source.0).unwrap();
        assert!(evaluate(&project, &project.root, 2).is_empty());
        let mut player = Player::new(&project);
        let old_a = player.child_path(&project, &[], "a").unwrap();
        let old_b = player.child_path(&project, &[], "b").unwrap();
        let old_life_a = player.lifetime_id(&old_a).unwrap();
        let old_life_b = player.lifetime_id(&old_b).unwrap();
        let stale = player.drain_scripts();
        assert!(player.set_property(&old_a, "x", 99.));
        assert!(player.control(&project, &old_b, PlaybackCommand::GotoAndStop(2)));
        assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(3)));
        let new_a = player.child_path(&project, &[], "a").unwrap();
        let new_b = player.child_path(&project, &[], "b").unwrap();
        assert_ne!(old_a, new_a);
        assert_ne!(old_b, new_b);
        assert_eq!(player.lifetime_id(&old_a), None);
        assert_eq!(player.lifetime_id(&old_b), None);
        assert_ne!(player.lifetime_id(&new_a), Some(old_life_a));
        assert_ne!(player.lifetime_id(&new_b), Some(old_life_b));
        assert_eq!(player.clip_frame(&new_a), Some(0));
        assert_eq!(player.clip_frame(&new_b), Some(0));
        assert_eq!(player.property(&project, &new_a, "x"), Some(35.));
        assert_eq!(player.property(&project, &new_b, "x"), Some(185.));
        assert!(
            stale
                .iter()
                .all(|event| !player.is_dispatch_current(&project, event))
        );
    }
}

#[test]
fn adjacent_same_symbol_keys_are_blocked_despite_names_or_reference_ids() {
    let instance = |name: &str, reference: &str| {
        format!(
            r#"<DOMSymbolInstance name="{name}" referenceID="{reference}" libraryItemName="Counter"/>"#
        )
    };
    let a = instance("a", "11111111-1111-1111-1111-111111111111");
    let b = instance("b", "22222222-2222-2222-2222-222222222222");
    let replacement = instance("a", "33333333-3333-3333-3333-333333333333");
    for (first, second) in [
        (format!("{a}{b}"), format!("{a}{b}")), // Same order/reference does not prove continuity.
        (format!("{a}{b}"), format!("{b}{a}")), // Two same-symbol placements reordered.
        (format!("{a}{b}"), format!("{replacement}{b}")), // Same-symbol replacement with new reference.
        (instance("a", ""), instance("a", "")),           // Names and XML slots are not identities.
    ] {
        let source = Source::new("instance-gap");
        source.write("DOMDocument.xml", &format!(r#"<DOMDocument><symbols><Include href="Counter.xml"/></symbols><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0" duration="2"><elements>{first}</elements></DOMFrame><DOMFrame index="2" duration="2"><elements>{second}</elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#));
        source.blocked("ambiguous_instance_continuity");
    }
}

#[test]
fn different_symbol_replacement_uses_new_ids_without_ambiguous_continuity() {
    let source = Source::new("instance-gap");
    let xml = fs::read_to_string(source.0.join("LIBRARY/Counter.xml"))
        .unwrap()
        .replace("name=\"Counter\"", "name=\"Other\"");
    source.write("LIBRARY/Other.xml", &xml);
    source.replace(
        "DOMDocument.xml",
        "<Include href=\"Counter.xml\"/>",
        "<Include href=\"Counter.xml\"/><Include href=\"Other.xml\"/>",
    );
    source.replace(
        "DOMDocument.xml",
        "index=\"0\" duration=\"2\"",
        "index=\"0\" duration=\"3\"",
    );
    source.replace(
        "DOMDocument.xml",
        "<DOMFrame index=\"2\"><elements/></DOMFrame>",
        "",
    );
    // Both old placements use Other; later Counter placements are unambiguously new symbols.
    source.replace(
        "DOMDocument.xml",
        "libraryItemName=\"Counter\"",
        "libraryItemName=\"Other\"",
    );
    source.replace(
        "DOMDocument.xml",
        "libraryItemName=\"Counter\"",
        "libraryItemName=\"Other\"",
    );
    let project = import_project(&source.0).unwrap();
    let mut player = Player::new(&project);
    let old = player.child_path(&project, &[], "a").unwrap();
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(3)));
    assert_ne!(old, player.child_path(&project, &[], "a").unwrap());
    assert_eq!(player.lifetime_id(&old), None);
}

#[test]
fn foreign_element_namespaces_cannot_alias_known_shapes_or_empty_containers() {
    for extra in [
        "<e:media xmlns:e=\"urn:unknown\"/>",
        "<e:fonts xmlns:e=\"urn:unknown\"/>",
        "<e:PrinterSettings xmlns:e=\"urn:unknown\"/>",
    ] {
        let source = Source::new("hidden-layers");
        source.replace(
            "DOMDocument.xml",
            "</DOMDocument>",
            &format!("{extra}</DOMDocument>"),
        );
        source.blocked("unknown_namespace");
    }
    let source = Source::new("hidden-layers");
    source.replace(
        "DOMDocument.xml",
        "<elements>",
        "<elements><e:DOMShape xmlns:e=\"urn:unknown\"/>",
    );
    source.blocked("unknown_namespace");
    let source = Source::new("hidden-layers");
    source.replace(
        "PublishSettings.xml",
        "</flash_profile>",
        "<e:Unused xmlns:e=\"urn:unknown\"/></flash_profile>",
    );
    source.blocked("unknown_namespace");
}

#[test]
fn canonical_adobe_element_prefix_is_allowed_alongside_unnamespaced_original_fixtures() {
    let source = Source::new("hidden-layers");
    source.replace(
        "DOMDocument.xml",
        "<DOMDocument width=",
        "<x:DOMDocument xmlns:x=\"http://ns.adobe.com/xfl/2008/\" width=",
    );
    source.replace("DOMDocument.xml", "</DOMDocument>", "</x:DOMDocument>");
    let project = import_project(&source.0).unwrap();
    assert_eq!(evaluate(&project, &project.root, 0).len(), 4);
}

#[test]
fn nested_profile_cannot_hide_a_policy_as_an_inactive_profile() {
    for nested in [
        "<flash_profile><InvisibleLayer>0</InvisibleLayer></flash_profile>",
        "<flash_profile current=\"false\"><InvisibleLayer>0</InvisibleLayer></flash_profile>",
        "<Unknown><flash_profile current=\"true\"><InvisibleLayer>0</InvisibleLayer></flash_profile></Unknown>",
    ] {
        let source = Source::new("hidden-layers");
        source.policy(nested);
        source.blocked("publish_profile");
    }
    let source = Source::new("hidden-layers");
    let active = profile("true", "2", "<InvisibleLayer>1</InvisibleLayer>");
    let inactive = profile(
        "false",
        "2",
        "<flash_profile><InvisibleLayer>0</InvisibleLayer></flash_profile>",
    );
    source.write(
        "PublishSettings.xml",
        &format!("<flash_profiles>{active}{inactive}</flash_profiles>"),
    );
    source.blocked("publish_profile");
}

#[test]
fn empty_media_and_font_markers_do_not_allow_unexamined_attributes_or_text() {
    for tag in ["media", "fonts"] {
        for (payload, code) in [
            (
                format!("<{tag} xmlns:e=\"urn:unknown\" e:payload=\"x\"/>"),
                "unsupported_attribute_namespace",
            ),
            (
                format!("<{tag} unexpected=\"x\"/>"),
                "unsupported_attribute",
            ),
            (
                format!("<{tag}>nonempty payload</{tag}>"),
                "unexpected_text",
            ),
            (
                format!("<{tag}><!-- split -->payload</{tag}>"),
                "unexpected_text",
            ),
            (format!("<{tag}><Unknown/></{tag}>"), "unsupported_assets"),
        ] {
            let source = Source::new("hidden-layers");
            source.replace(
                "DOMDocument.xml",
                "</DOMDocument>",
                &format!("{payload}</DOMDocument>"),
            );
            source.blocked(code);
        }
        let source = Source::new("hidden-layers");
        source.replace(
            "DOMDocument.xml",
            "</DOMDocument>",
            &format!("<{tag}> <!-- empty --> </{tag}></DOMDocument>"),
        );
        let project = import_project(&source.0).unwrap();
        assert_eq!(evaluate(&project, &project.root, 0).len(), 4);
    }
}
