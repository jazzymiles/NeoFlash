use neoflash_core::{Script, ScriptLanguage};

fn compile(
    source: &str,
    language: ScriptLanguage,
) -> Result<String, Vec<neoflash_script::Diagnostic>> {
    neoflash_script::compile(&Script {
        language,
        source: source.into(),
    })
}

#[test]
fn dynamic_bare_timeline_calls_are_an_explicit_as2_boundary() {
    for source in ["run();", "function forward() { run(); } forward();"] {
        assert!(compile(source, ScriptLanguage::ActionScript2).is_ok());
        assert!(compile(source, ScriptLanguage::ActionScript3).is_err());
    }
    for source in ["child.run();", "child['run']();", "new Missing();"] {
        assert!(
            compile(source, ScriptLanguage::ActionScript2).is_err(),
            "{source}"
        );
    }
    assert!(
        compile(
            "function f(__nfAs2Method) {}",
            ScriptLanguage::ActionScript2
        )
        .is_err()
    );
}

#[test]
fn as2_goto_aliases_are_diagnosed_without_blocking_local_shadowing() {
    for source in [
        "var jump = gotoAndStop; jump('END');",
        "var jump = clip.gotoAndStop; jump('END');",
        "var jump = this.gotoAndPlay; jump('END');",
    ] {
        let error = compile(source, ScriptLanguage::ActionScript2).unwrap_err();
        assert!(
            error[0].message.contains("goto method values and aliases"),
            "{source}: {error:?}"
        );
        assert!(compile(source, ScriptLanguage::ActionScript3).is_ok());
    }
    for source in [
        "function call(gotoAndStop) { var jump = gotoAndStop; jump('END'); } call(function(value) { trace(value); });",
        "var tools = {gotoAndStop:function(value) { trace(value); }}; var jump = tools.gotoAndStop; jump('END');",
    ] {
        assert!(
            compile(source, ScriptLanguage::ActionScript2).is_ok(),
            "{source}"
        );
    }
}
