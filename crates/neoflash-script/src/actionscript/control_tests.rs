use super::*;

fn check(source: &str, language: ScriptLanguage) -> Result<String, Diagnostic> {
    compile(&Script {
        language,
        source: source.into(),
    })
}
fn as3(source: &str) -> Result<String, Diagnostic> {
    check(source, ScriptLanguage::ActionScript3)
}

#[test]
fn control_targets_are_validated_at_source_and_reset_at_functions() {
    for source in [
        "break;",
        "continue;",
        "switch(1){case 1:continue;}",
        "outer:{continue outer;}",
        "outer:while(true){break missing;}",
        "outer:while(true){outer:{break;}}",
        "while(true){var f=function(){break;};}",
        "outer:while(true){var f=function(){continue outer;};}",
        "switch(1){default:break;default:break;}",
        "switch(1){trace(1);case 1:break;}",
        "case 1: break;",
        "var switch=1;",
        "var try=1;",
        "for each(var key=1;key<3;key++){}",
        "for(var key=1 in {}){}",
        "for(var a,b in {}){}",
        "for(Number in {}){}",
        "delete local;",
        "var value:Vector.<void>;",
        "var value:Vector;",
        "tree.(value==1);",
        "delete tree..item;",
        "tree..item='value';",
        "var tree=<root>line\ntext</root> var n=1;",
        "default xml namespace = \"urn:catalog\";",
    ] {
        assert!(as3(source).is_err(), "must reject {source}");
    }
    for source in [
        "outer:for(;;){switch(1){case 1:continue outer;default:break;}break;}",
        "first:second:do{continue first;}while(false);",
        "block:{break block;}",
        "switch(1){case 1:break;} while(false){continue;}",
        "outer:for each(var value:int in new Vector.<int>(2)){try{continue outer;}finally{trace(value);}}",
        "for(var key:String in {a:1}){if(key in {a:2})delete this[key];}",
        "var value:Vector.<Vector.<int>>=new Vector.<Vector.<int>>(); var shifts=(16>>2)+(16>>>3);",
        "for(var f=function(){return 'x' in {x:1};};false;){}",
        "var tree:XML=<root a={String(1 < 2)}>{(function():XML {return <child>We're fine.</child>;})()}</root>; var values:Vector.<XML>=new Vector.<XML>(); values.push(tree); var shifts=(32>>2)+(32>>>3);",
        "var ns:Namespace=new Namespace('m','urn:m');var q:QName=new QName(ns,'item');tree.ns::item;tree.@[key]='v';delete tree.@id;tree.*;tree..*;tree..@id;tree[q];",
        "function shadow(Namespace:Function,QName:Function):String{return Namespace('a')+QName('b');}",
        "var tree:XML=XML('<root/>'); var list:XMLList=new XMLList('<item/>'); for each(var item:XML in list){trace(String(item));} var yes:Boolean=tree is XML;var optional:XML=tree as XML;",
        "var values:Vector.<XML>=new Vector.<XML>();values.push(new XML('<root/>'));",
        "function local(XML:Function,XMLList:Function):String {return String(XML('x'))+String(XMLList('y'));} var result=local(function(x:String):String{return x;},function(x:String):String{return x;});",
    ] {
        assert!(as3(source).is_ok(), "must accept {source}");
    }
    assert!(check("label:while(false){}", ScriptLanguage::ActionScript2).is_err());
    assert!(
        check(
            "for each(var value in [1]){}",
            ScriptLanguage::ActionScript2
        )
        .is_err()
    );
    assert!(check("var value:Vector.<int>;", ScriptLanguage::ActionScript2).is_err());
    assert!(check("var tree=<root/>;", ScriptLanguage::ActionScript2).is_err());
    assert!(check("tree.@id;", ScriptLanguage::ActionScript2).is_err());
}

#[test]
fn throw_and_try_syntax_diagnostics_use_original_locations() {
    for source in [
        "throw;",
        "throw\n1;",
        "throw\u{2028}1;",
        "throw/* newline\n*/1;",
        "try{}",
        "try{}catch(){}",
        "try{}catch(e:Missing){}",
        "try{}finally{}catch(e){}",
        "try{}catch(e:*){}catch(f:Error){}",
        "try{}finally{}finally{}",
    ] {
        assert!(as3(source).is_err(), "must reject {source}");
    }
    let error = as3("var n=0;\nthrow\n1;").unwrap_err();
    assert_eq!(error.language, ScriptLanguage::ActionScript3);
    assert_eq!((error.line, error.column), (3, 1));
    let error = as3("var n=0;\ntry {} catch (e:Missing) {}").unwrap_err();
    assert_eq!(error.line, 2);
}

#[test]
fn completion_accounts_for_throw_catches_switch_and_finally_overrides() {
    for source in [
        "function f():int {throw 1;}",
        "function f():int {try{return 1;}finally{trace(2);}}",
        "function f():int {try{trace(1);}finally{return 2;}}",
        "function f():int {try{throw 1;}catch(e:*){return 2;}}",
        "function f(x:int):int {switch(x){case 1:return 1;default:throw 2;}}",
        "function f():int {do{return 1;}while(false);}",
    ] {
        assert!(as3(source).is_ok(), "must accept {source}");
    }
    for source in [
        "function f():int {try{return 1;}catch(e:*){trace(e);}}",
        "function f(x:int):int {switch(x){case 1:return 1;}}",
        "function f(x:int):int {switch(x){default:break;}}",
        "function f():int {do{continue;}while(false);}",
        "function f():int {done:{try{return 1;}finally{break done;}}}",
        "function f():int {try{throw 1;}catch(e:*){return;}}",
    ] {
        assert!(as3(source).is_err(), "must reject {source}");
    }
}

#[test]
fn catch_scope_preserves_call_resolution_and_unsupported_boundaries() {
    assert!(as3("var tools={};try{throw {};}catch(tools:*){tools.run=function(){return 1;};} tools.run();").is_err());
    assert!(as3("try{throw {};}catch(e:*){e.run=function(){return 1;};result=e.run();}").is_ok());
    assert!(as3("try{throw {};}catch(e:*){e.run=function(){return 1;};} other.run();").is_err());
    assert!(as3("try{throw 1;}catch(e:int){e();}").is_err());
    assert!(as3("try{throw 1;}catch(e:*){function f(){}}").is_err());
    assert!(as3("function f(Error:Function){return new Error();}").is_err());
    assert!(as3("var constructor=Error;").is_err());
    assert!(as3("new Custom();").is_err());
    assert!(as3("new Error(1,2,3);").is_err());
    assert!(as3("try{throw 1;}catch(__nfValue:*){}").is_err());
}

#[test]
fn as2_catch_scope_and_type_gates_are_explicit() {
    for source in [
        "try{throw 1;}catch(e:String){}",
        "try{throw 1;}catch(e:*){}",
        "var e;try{throw 1;}catch(e){}",
        "try{throw 1;}catch(e){}trace(e);",
        "try{throw 1;}catch(Number){}",
        "try{throw 1;}catch(e){callback=function(){return e;};}",
        "new TypeError();",
        "new Error(1,2);",
    ] {
        assert!(
            check(source, ScriptLanguage::ActionScript2).is_err(),
            "must reject {source}"
        );
    }
    assert!(
        check(
            "try{throw 1;}catch(e){value=e;}finally{cleaned=true;}",
            ScriptLanguage::ActionScript2
        )
        .is_ok()
    );
}

#[test]
fn structured_nesting_limits_still_fail_before_stack_exhaustion() {
    let source = format!("{}throw 1;{}", "try{".repeat(200), "}finally{}".repeat(200));
    assert!(as3(&source).is_err());
}
