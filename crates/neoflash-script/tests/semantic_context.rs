use neoflash_core::{Project, ScriptLanguage, SourceUnit};
use neoflash_script::language_service::{HintContext, ThisKind};

fn factory_source(result_type: &str) -> String {
    format!(
        "package demo {{ import flash.geom.{result_type}; public class Factory {{ public function create():{result_type} {{ return new {result_type}(); }} }} }}"
    )
}

#[test]
fn semantic_snapshots_follow_project_signatures_without_reusing_stale_receiver_types() {
    let mut project = Project::blank();
    project.sources.push(SourceUnit {
        id: "factory-flow-source".into(),
        path: "demo/Factory.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: factory_source("Point"),
    });
    let handler = "import demo.Factory; var factory:Factory = new Factory(); factory.create().";
    let caret = handler.chars().count();
    neoflash_script::compile_project(&project).unwrap();
    let context =
        HintContext::new(&project, ScriptLanguage::ActionScript3, ThisKind::MovieClip).unwrap();
    let analysis = context.semantic_analysis(handler);
    assert!(analysis.is_analyzed(caret), "imports reach the AST service");
    assert_eq!(
        analysis.type_at("factory", caret).as_deref(),
        Some("demo.Factory")
    );
    let hints = context.hints(handler, caret, true).unwrap();
    assert!(hints.items.iter().any(|hint| hint.label == "length"));
    assert!(!hints.items.iter().any(|hint| hint.label == "bottomRight"));

    // The queried handler is byte-for-byte identical. Only its project dependency changes.
    project.sources[0].source = factory_source("Rectangle");
    neoflash_script::compile_project(&project).unwrap();
    let updated =
        HintContext::new(&project, ScriptLanguage::ActionScript3, ThisKind::MovieClip).unwrap();
    let hints = updated.hints(handler, caret, true).unwrap();
    assert!(hints.items.iter().any(|hint| hint.label == "bottomRight"));
    assert!(!hints.items.iter().any(|hint| hint.label == "length"));
    // An existing immutable context remains a consistent snapshot of its earlier source graph.
    assert!(
        context
            .hints(handler, caret, true)
            .unwrap()
            .items
            .iter()
            .any(|hint| hint.label == "length")
    );

    let query = "function make() { return new Rectangle(); } function unrelated():void { var hidden:Point; } var result = make(); result.";
    assert_eq!(
        updated
            .semantic_analysis(query)
            .type_at("result", query.chars().count())
            .as_deref(),
        Some("flash.geom.Rectangle"),
        "factory facts come from the compiler AST"
    );
    let hints = updated.hints(query, query.chars().count(), true).unwrap();
    assert!(
        hints.items.iter().any(|hint| hint.label == "bottomRight"),
        "function-result inference reaches production completions"
    );
    let unknown = "function unrelated():void { var hidden:Point; } hidden.";
    assert!(
        updated
            .hints(unknown, unknown.chars().count(), true)
            .is_none(),
        "sibling/local declarations cannot leak into timeline scope"
    );
}
