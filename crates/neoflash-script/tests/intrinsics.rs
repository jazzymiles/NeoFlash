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
fn numeric_method_values_and_declared_class_aliases_compile_with_bounded_runtime_dispatch() {
    for source in [
        "var callback = (1).toFixed; callback(21);",
        "var callback = (1)['toFixed']; callback(21);",
        "var callback = (1)['to\\u0046ixed']; callback(21);",
        "var numeric:Class=int; var value=new numeric('42'); var bound=int.MAX_VALUE;",
        "function construct(numeric:Class):Number {return new numeric('42');} result=construct(Number);",
    ] {
        assert!(
            compile(source, ScriptLanguage::ActionScript3).is_ok(),
            "{source}"
        );
    }
    assert!(
        compile(
            "var key='toFixed'; (1)[key](21);",
            ScriptLanguage::ActionScript3
        )
        .is_err()
    );
}

#[test]
fn custom_fixed_methods_and_shadowed_parse_int_remain_supported() {
    for language in [ScriptLanguage::ActionScript2, ScriptLanguage::ActionScript3] {
        for source in [
            "var parse:Function=parseInt; result=parse('0777');",
            "function call(parseInt:Function) { return parseInt('0777'); } call(function(v) { return v; });",
            "var custom={parseInt:function(v) {return v;}}; result=custom.parseInt('0777');",
            "var custom={toFixed:function(v) {return v;}}; var callback=custom.toFixed; result=callback(21);",
        ] {
            assert!(compile(source, language).is_ok(), "{source}");
        }
    }
}

#[test]
fn compiler_intrinsic_identifiers_are_reserved() {
    for name in [
        "__nfParseInt2",
        "__nfParseInt3",
        "__nfFixedMethod",
        "__nfNativeFixed",
        "__nfApply",
        "__nfMethodValue",
        "__nfPropertyValue",
    ] {
        assert!(
            compile(
                &format!("function f({name}) {{ return 0; }}"),
                ScriptLanguage::ActionScript3
            )
            .is_err()
        );
    }
}

#[test]
fn excess_parse_int_arguments_are_a_named_boundary_not_a_flash_error_claim() {
    let errors = compile("parseInt('10', 10, 0);", ScriptLanguage::ActionScript3).unwrap_err();
    assert!(
        errors[0]
            .message
            .contains("excess-argument semantics are not verified")
    );
    assert!(compile("parseInt('10', 10, 0);", ScriptLanguage::ActionScript2).is_ok());
    assert!(
        compile(
            "function invoke(parseInt:Function) { return parseInt('10',10,0); }",
            ScriptLanguage::ActionScript3
        )
        .is_ok()
    );
}

#[test]
fn core_syntax_boundaries_keep_actionable_diagnostics() {
    for source in [
        "var expression=/unterminated;",
        "var expression=/[a/;",
        "var expression=/a/u;",
        "var expression=/a\nb/;",
        "function local(Number) { return 1 is Number; }",
        "function local(Object) { return new Object(); }",
        "var value=new Number(1,2);",
        "var invalid=(1,2)=3;",
        "for(var i=0,j=0 in {}) {}",
    ] {
        assert!(
            compile(source, ScriptLanguage::ActionScript3).is_err(),
            "{source}"
        );
    }
    for source in [
        "var expression=/a/;",
        "var value=new Camera();",
        "var value=1 is Number;",
    ] {
        assert!(
            compile(source, ScriptLanguage::ActionScript2).is_err(),
            "{source}"
        );
    }
}
