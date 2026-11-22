//! Deterministic Xcode `project.pbxproj` (`objectVersion = 56`) and shared `.xcscheme` generator (`T-455`).

use crate::sha256_hex;
use std::fmt::Write as _;

pub(super) struct PbxProjectSpec<'a> {
    pub target_name: &'a str,
    pub bundle_identifier: &'a str,
    pub app_swift_files: &'a [String],
    pub kit_swift_files: &'a [String],
    pub scenes_swift_files: &'a [String],
    pub resource_files: &'a [String],
}

/// Deterministic 24-hex-character uppercase object ID from a semantic seed.
#[must_use]
pub(super) fn pbx_object_id(seed: &str) -> String {
    let hex = sha256_hex(format!("neoflash.xcode.pbx.{seed}").as_bytes());
    hex[..24].to_ascii_uppercase()
}

fn pbx_str(s: &str) -> String {
    let safe_unquoted = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/'));
    if safe_unquoted {
        s.to_owned()
    } else {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for ch in s.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                other => out.push(other),
            }
        }
        out.push('"');
        out
    }
}

fn xcode_file_type(filename: &str) -> &'static str {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "swift" => "sourcecode.swift",
        "plist" => "text.plist.xml",
        "png" => "image.png",
        "jpg" | "jpeg" => "image.jpeg",
        "gif" => "image.gif",
        "wav" => "audio.wav",
        "mp3" => "audio.mp3",
        "m4a" | "aac" => "audio.mp4",
        "mp4" | "m4v" => "video.mpeg4",
        "mov" => "video.quicktime",
        _ => "file",
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[derive(Clone)]
struct FileEntry {
    file_ref_id: String,
    build_file_id: Option<String>,
    filename: String,
    file_type: &'static str,
    phase_comment: &'static str,
}

pub(super) fn generate_pbxproj(spec: &PbxProjectSpec<'_>) -> String {
    let project_id = pbx_object_id(&format!("project:{}", spec.target_name));
    let target_id = pbx_object_id(&format!("target:{}", spec.target_name));
    let product_ref_id = pbx_object_id(&format!("product:{}.app", spec.target_name));

    let main_group_id = pbx_object_id("group:main");
    let app_group_id = pbx_object_id("group:App");
    let generated_group_id = pbx_object_id("group:Generated");
    let kit_group_id = pbx_object_id("group:Generated/NeoFlashKit");
    let scenes_group_id = pbx_object_id("group:Generated/Scenes");
    let resources_group_id = pbx_object_id("group:Generated/Resources");
    let products_group_id = pbx_object_id("group:Products");

    let sources_phase_id = pbx_object_id("phase:Sources");
    let frameworks_phase_id = pbx_object_id("phase:Frameworks");
    let resources_phase_id = pbx_object_id("phase:Resources");

    let proj_cfg_list_id = pbx_object_id("cfglist:project");
    let proj_debug_cfg_id = pbx_object_id("cfg:project:Debug");
    let proj_release_cfg_id = pbx_object_id("cfg:project:Release");

    let target_cfg_list_id = pbx_object_id("cfglist:target");
    let target_debug_cfg_id = pbx_object_id("cfg:target:Debug");
    let target_release_cfg_id = pbx_object_id("cfg:target:Release");

    let mut all_files: Vec<FileEntry> = Vec::new();
    let mut app_file_refs = Vec::new();
    let mut kit_file_refs = Vec::new();
    let mut scenes_file_refs = Vec::new();
    let mut resource_group_refs = Vec::new();
    let mut source_build_files = Vec::new();
    let mut resource_build_files = Vec::new();

    for name in spec.app_swift_files {
        let rel = format!("App/{name}");
        let file_ref_id = pbx_object_id(&format!("fileref:{rel}"));
        let build_file_id = pbx_object_id(&format!("buildfile:{rel}"));
        app_file_refs.push((file_ref_id.clone(), name.clone()));
        source_build_files.push((build_file_id.clone(), name.clone()));
        all_files.push(FileEntry {
            file_ref_id,
            build_file_id: Some(build_file_id),
            filename: name.clone(),
            file_type: xcode_file_type(name),
            phase_comment: "Sources",
        });
    }

    for name in spec.kit_swift_files {
        let rel = format!("Generated/NeoFlashKit/{name}");
        let file_ref_id = pbx_object_id(&format!("fileref:{rel}"));
        let build_file_id = pbx_object_id(&format!("buildfile:{rel}"));
        kit_file_refs.push((file_ref_id.clone(), name.clone()));
        source_build_files.push((build_file_id.clone(), name.clone()));
        all_files.push(FileEntry {
            file_ref_id,
            build_file_id: Some(build_file_id),
            filename: name.clone(),
            file_type: xcode_file_type(name),
            phase_comment: "Sources",
        });
    }

    for name in spec.scenes_swift_files {
        let rel = format!("Generated/Scenes/{name}");
        let file_ref_id = pbx_object_id(&format!("fileref:{rel}"));
        let build_file_id = pbx_object_id(&format!("buildfile:{rel}"));
        scenes_file_refs.push((file_ref_id.clone(), name.clone()));
        source_build_files.push((build_file_id.clone(), name.clone()));
        all_files.push(FileEntry {
            file_ref_id,
            build_file_id: Some(build_file_id),
            filename: name.clone(),
            file_type: xcode_file_type(name),
            phase_comment: "Sources",
        });
    }

    // Info.plist is referenced by INFOPLIST_FILE (not copied in PBXResourcesBuildPhase).
    let info_plist_ref_id = pbx_object_id("fileref:Generated/Resources/Info.plist");
    resource_group_refs.push((info_plist_ref_id.clone(), "Info.plist".to_owned()));
    all_files.push(FileEntry {
        file_ref_id: info_plist_ref_id,
        build_file_id: None,
        filename: "Info.plist".to_owned(),
        file_type: "text.plist.xml",
        phase_comment: "",
    });

    for name in spec.resource_files {
        let rel = format!("Generated/Resources/{name}");
        let file_ref_id = pbx_object_id(&format!("fileref:{rel}"));
        let build_file_id = pbx_object_id(&format!("buildfile:{rel}"));
        resource_group_refs.push((file_ref_id.clone(), name.clone()));
        resource_build_files.push((build_file_id.clone(), name.clone()));
        all_files.push(FileEntry {
            file_ref_id,
            build_file_id: Some(build_file_id),
            filename: name.clone(),
            file_type: xcode_file_type(name),
            phase_comment: "Resources",
        });
    }

    let app_product_name = format!("{}.app", spec.target_name);
    let mut out = String::with_capacity(8192);
    let _ = writeln!(out, "// !$*UTF8*$!");
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "\tarchiveVersion = 1;");
    let _ = writeln!(out, "\tclasses = {{");
    let _ = writeln!(out, "\t}};");
    let _ = writeln!(out, "\tobjectVersion = 56;");
    let _ = writeln!(out, "\tobjects = {{");
    let _ = writeln!(out);

    // Begin PBXBuildFile section
    let _ = writeln!(out, "/* Begin PBXBuildFile section */");
    for entry in &all_files {
        if let Some(build_id) = &entry.build_file_id {
            let _ = writeln!(
                out,
                "\t\t{build_id} /* {} in {} */ = {{isa = PBXBuildFile; fileRef = {} /* {} */; }};",
                entry.filename, entry.phase_comment, entry.file_ref_id, entry.filename
            );
        }
    }
    let _ = writeln!(out, "/* End PBXBuildFile section */");
    let _ = writeln!(out);

    // Begin PBXFileReference section
    let _ = writeln!(out, "/* Begin PBXFileReference section */");
    let _ = writeln!(
        out,
        "\t\t{product_ref_id} /* {app_product_name} */ = {{isa = PBXFileReference; explicitFileType = wrapper.application; includeInIndex = 0; path = {}; sourceTree = BUILT_PRODUCTS_DIR; }};",
        pbx_str(&app_product_name)
    );
    for entry in &all_files {
        let _ = writeln!(
            out,
            "\t\t{} /* {} */ = {{isa = PBXFileReference; lastKnownFileType = {}; path = {}; sourceTree = \"<group>\"; }};",
            entry.file_ref_id,
            entry.filename,
            pbx_str(entry.file_type),
            pbx_str(&entry.filename)
        );
    }
    let _ = writeln!(out, "/* End PBXFileReference section */");
    let _ = writeln!(out);

    // Begin PBXFrameworksBuildPhase section
    let _ = writeln!(out, "/* Begin PBXFrameworksBuildPhase section */");
    let _ = writeln!(
        out,
        "\t\t{frameworks_phase_id} /* Frameworks */ = {{\n\t\t\tisa = PBXFrameworksBuildPhase;\n\t\t\tbuildActionMask = 2147483647;\n\t\t\tfiles = (\n\t\t\t);\n\t\t\trunOnlyForDeploymentPostprocessing = 0;\n\t\t}};"
    );
    let _ = writeln!(out, "/* End PBXFrameworksBuildPhase section */");
    let _ = writeln!(out);

    // Begin PBXGroup section
    let _ = writeln!(out, "/* Begin PBXGroup section */");
    let _ = writeln!(out, "\t\t{main_group_id} = {{");
    let _ = writeln!(out, "\t\t\tisa = PBXGroup;");
    let _ = writeln!(out, "\t\t\tchildren = (");
    let _ = writeln!(out, "\t\t\t\t{app_group_id} /* App */,");
    let _ = writeln!(out, "\t\t\t\t{generated_group_id} /* Generated */,");
    let _ = writeln!(out, "\t\t\t\t{products_group_id} /* Products */,");
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\tsourceTree = \"<group>\";");
    let _ = writeln!(out, "\t\t}};");

    write_pbx_group(&mut out, &app_group_id, "App", Some("App"), &app_file_refs);

    let _ = writeln!(out, "\t\t{generated_group_id} /* Generated */ = {{");
    let _ = writeln!(out, "\t\t\tisa = PBXGroup;");
    let _ = writeln!(out, "\t\t\tchildren = (");
    let _ = writeln!(out, "\t\t\t\t{kit_group_id} /* NeoFlashKit */,");
    let _ = writeln!(out, "\t\t\t\t{scenes_group_id} /* Scenes */,");
    let _ = writeln!(out, "\t\t\t\t{resources_group_id} /* Resources */,");
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\tpath = Generated;");
    let _ = writeln!(out, "\t\t\tsourceTree = \"<group>\";");
    let _ = writeln!(out, "\t\t}};");

    write_pbx_group(
        &mut out,
        &kit_group_id,
        "NeoFlashKit",
        Some("NeoFlashKit"),
        &kit_file_refs,
    );
    write_pbx_group(
        &mut out,
        &scenes_group_id,
        "Scenes",
        Some("Scenes"),
        &scenes_file_refs,
    );
    write_pbx_group(
        &mut out,
        &resources_group_id,
        "Resources",
        Some("Resources"),
        &resource_group_refs,
    );
    write_pbx_group(
        &mut out,
        &products_group_id,
        "Products",
        None,
        &[(product_ref_id.clone(), app_product_name)],
    );
    let _ = writeln!(out, "/* End PBXGroup section */");
    let _ = writeln!(out);

    // Begin PBXNativeTarget section
    let _ = writeln!(out, "/* Begin PBXNativeTarget section */");
    let _ = writeln!(out, "\t\t{target_id} /* {} */ = {{", spec.target_name);
    let _ = writeln!(out, "\t\t\tisa = PBXNativeTarget;");
    let _ = writeln!(
        out,
        "\t\t\tbuildConfigurationList = {target_cfg_list_id} /* Build configuration list for PBXNativeTarget \"{}\" */;",
        spec.target_name
    );
    let _ = writeln!(out, "\t\t\tbuildPhases = (");
    let _ = writeln!(out, "\t\t\t\t{sources_phase_id} /* Sources */,");
    let _ = writeln!(out, "\t\t\t\t{frameworks_phase_id} /* Frameworks */,");
    let _ = writeln!(out, "\t\t\t\t{resources_phase_id} /* Resources */,");
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\tbuildRules = (");
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\tdependencies = (");
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\tname = {};", pbx_str(spec.target_name));
    let _ = writeln!(out, "\t\t\tproductName = {};", pbx_str(spec.target_name));
    let _ = writeln!(
        out,
        "\t\t\tproductReference = {product_ref_id} /* {}.app */;",
        spec.target_name
    );
    let _ = writeln!(
        out,
        "\t\t\tproductType = \"com.apple.product-type.application\";"
    );
    let _ = writeln!(out, "\t\t}};");
    let _ = writeln!(out, "/* End PBXNativeTarget section */");
    let _ = writeln!(out);

    // Begin PBXProject section
    let _ = writeln!(out, "/* Begin PBXProject section */");
    let _ = writeln!(out, "\t\t{project_id} /* Project object */ = {{");
    let _ = writeln!(out, "\t\t\tisa = PBXProject;");
    let _ = writeln!(out, "\t\t\tattributes = {{");
    let _ = writeln!(out, "\t\t\t\tBuildIndependentTargetsInParallel = 1;");
    let _ = writeln!(out, "\t\t\t\tLastSwiftUpdateCheck = 1600;");
    let _ = writeln!(out, "\t\t\t\tLastUpgradeCheck = 1600;");
    let _ = writeln!(out, "\t\t\t}};");
    let _ = writeln!(
        out,
        "\t\t\tbuildConfigurationList = {proj_cfg_list_id} /* Build configuration list for PBXProject \"{}\" */;",
        spec.target_name
    );
    let _ = writeln!(out, "\t\t\tcompatibilityVersion = \"Xcode 14.0\";");
    let _ = writeln!(out, "\t\t\tdevelopmentRegion = en;");
    let _ = writeln!(out, "\t\t\thasScannedForEncodings = 0;");
    let _ = writeln!(out, "\t\t\tknownRegions = (");
    let _ = writeln!(out, "\t\t\t\ten,");
    let _ = writeln!(out, "\t\t\t\tBase,");
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\tmainGroup = {main_group_id};");
    let _ = writeln!(
        out,
        "\t\t\tproductRefGroup = {products_group_id} /* Products */;"
    );
    let _ = writeln!(out, "\t\t\tprojectDirPath = \"\";");
    let _ = writeln!(out, "\t\t\tprojectRoot = \"\";");
    let _ = writeln!(out, "\t\t\ttargets = (");
    let _ = writeln!(out, "\t\t\t\t{target_id} /* {} */,", spec.target_name);
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t}};");
    let _ = writeln!(out, "/* End PBXProject section */");
    let _ = writeln!(out);

    // Begin PBXResourcesBuildPhase section
    let _ = writeln!(out, "/* Begin PBXResourcesBuildPhase section */");
    let _ = writeln!(out, "\t\t{resources_phase_id} /* Resources */ = {{");
    let _ = writeln!(out, "\t\t\tisa = PBXResourcesBuildPhase;");
    let _ = writeln!(out, "\t\t\tbuildActionMask = 2147483647;");
    let _ = writeln!(out, "\t\t\tfiles = (");
    for (bid, name) in &resource_build_files {
        let _ = writeln!(out, "\t\t\t\t{bid} /* {name} in Resources */,");
    }
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\trunOnlyForDeploymentPostprocessing = 0;");
    let _ = writeln!(out, "\t\t}};");
    let _ = writeln!(out, "/* End PBXResourcesBuildPhase section */");
    let _ = writeln!(out);

    // Begin PBXSourcesBuildPhase section
    let _ = writeln!(out, "/* Begin PBXSourcesBuildPhase section */");
    let _ = writeln!(out, "\t\t{sources_phase_id} /* Sources */ = {{");
    let _ = writeln!(out, "\t\t\tisa = PBXSourcesBuildPhase;");
    let _ = writeln!(out, "\t\t\tbuildActionMask = 2147483647;");
    let _ = writeln!(out, "\t\t\tfiles = (");
    for (bid, name) in &source_build_files {
        let _ = writeln!(out, "\t\t\t\t{bid} /* {name} in Sources */,");
    }
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\trunOnlyForDeploymentPostprocessing = 0;");
    let _ = writeln!(out, "\t\t}};");
    let _ = writeln!(out, "/* End PBXSourcesBuildPhase section */");
    let _ = writeln!(out);

    // Begin XCBuildConfiguration section
    let _ = writeln!(out, "/* Begin XCBuildConfiguration section */");
    write_project_build_config(&mut out, &proj_debug_cfg_id, "Debug", true);
    write_project_build_config(&mut out, &proj_release_cfg_id, "Release", false);
    write_target_build_config(
        &mut out,
        &target_debug_cfg_id,
        "Debug",
        spec.bundle_identifier,
        true,
    );
    write_target_build_config(
        &mut out,
        &target_release_cfg_id,
        "Release",
        spec.bundle_identifier,
        false,
    );
    let _ = writeln!(out, "/* End XCBuildConfiguration section */");
    let _ = writeln!(out);

    // Begin XCConfigurationList section
    let _ = writeln!(out, "/* Begin XCConfigurationList section */");
    let _ = writeln!(
        out,
        "\t\t{proj_cfg_list_id} /* Build configuration list for PBXProject \"{}\" */ = {{",
        spec.target_name
    );
    let _ = writeln!(out, "\t\t\tisa = XCConfigurationList;");
    let _ = writeln!(out, "\t\t\tbuildConfigurations = (");
    let _ = writeln!(out, "\t\t\t\t{proj_debug_cfg_id} /* Debug */,");
    let _ = writeln!(out, "\t\t\t\t{proj_release_cfg_id} /* Release */,");
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\tdefaultConfigurationIsVisible = 0;");
    let _ = writeln!(out, "\t\t\tdefaultConfigurationName = Release;");
    let _ = writeln!(out, "\t\t}};");

    let _ = writeln!(
        out,
        "\t\t{target_cfg_list_id} /* Build configuration list for PBXNativeTarget \"{}\" */ = {{",
        spec.target_name
    );
    let _ = writeln!(out, "\t\t\tisa = XCConfigurationList;");
    let _ = writeln!(out, "\t\t\tbuildConfigurations = (");
    let _ = writeln!(out, "\t\t\t\t{target_debug_cfg_id} /* Debug */,");
    let _ = writeln!(out, "\t\t\t\t{target_release_cfg_id} /* Release */,");
    let _ = writeln!(out, "\t\t\t);");
    let _ = writeln!(out, "\t\t\tdefaultConfigurationIsVisible = 0;");
    let _ = writeln!(out, "\t\t\tdefaultConfigurationName = Release;");
    let _ = writeln!(out, "\t\t}};");
    let _ = writeln!(out, "/* End XCConfigurationList section */");

    let _ = writeln!(out, "\t}};");
    let _ = writeln!(out, "\trootObject = {project_id} /* Project object */;");
    let _ = writeln!(out, "}}");
    out
}

fn write_pbx_group(
    out: &mut String,
    group_id: &str,
    label: &str,
    path: Option<&str>,
    children: &[(String, String)],
) {
    let _ = writeln!(out, "\t\t{group_id} /* {label} */ = {{");
    let _ = writeln!(out, "\t\t\tisa = PBXGroup;");
    let _ = writeln!(out, "\t\t\tchildren = (");
    for (cid, cname) in children {
        let _ = writeln!(out, "\t\t\t\t{cid} /* {cname} */,");
    }
    let _ = writeln!(out, "\t\t\t);");
    if let Some(p) = path {
        let _ = writeln!(out, "\t\t\tpath = {};", pbx_str(p));
    } else {
        let _ = writeln!(out, "\t\t\tname = {};", pbx_str(label));
    }
    let _ = writeln!(out, "\t\t\tsourceTree = \"<group>\";");
    let _ = writeln!(out, "\t\t}};");
}

fn write_project_build_config(out: &mut String, cfg_id: &str, name: &str, debug: bool) {
    let _ = writeln!(out, "\t\t{cfg_id} /* {name} */ = {{");
    let _ = writeln!(out, "\t\t\tisa = XCBuildConfiguration;");
    let _ = writeln!(out, "\t\t\tbuildSettings = {{");
    let _ = writeln!(out, "\t\t\t\tALWAYS_SEARCH_USER_PATHS = NO;");
    let _ = writeln!(out, "\t\t\t\tCLANG_ENABLE_MODULES = YES;");
    let _ = writeln!(out, "\t\t\t\tCLANG_ENABLE_OBJC_ARC = YES;");
    let _ = writeln!(out, "\t\t\t\tENABLE_USER_SCRIPT_SANDBOXING = YES;");
    let _ = writeln!(out, "\t\t\t\tIPHONEOS_DEPLOYMENT_TARGET = 17.0;");
    let _ = writeln!(out, "\t\t\t\tMACOSX_DEPLOYMENT_TARGET = 15.0;");
    let _ = writeln!(out, "\t\t\t\tSDKROOT = auto;");
    let _ = writeln!(out, "\t\t\t\tSWIFT_STRICT_CONCURRENCY = complete;");
    let _ = writeln!(out, "\t\t\t\tSWIFT_VERSION = 6.0;");
    if debug {
        let _ = writeln!(out, "\t\t\t\tGCC_OPTIMIZATION_LEVEL = 0;");
        let _ = writeln!(out, "\t\t\t\tONLY_ACTIVE_ARCH = YES;");
        let _ = writeln!(
            out,
            "\t\t\t\tSWIFT_ACTIVE_COMPILATION_CONDITIONS = \"DEBUG $(inherited)\";"
        );
        let _ = writeln!(out, "\t\t\t\tSWIFT_OPTIMIZATION_LEVEL = \"-Onone\";");
    } else {
        let _ = writeln!(out, "\t\t\t\tSWIFT_COMPILATION_MODE = wholemodule;");
        let _ = writeln!(out, "\t\t\t\tSWIFT_OPTIMIZATION_LEVEL = \"-O\";");
    }
    let _ = writeln!(out, "\t\t\t}};");
    let _ = writeln!(out, "\t\t\tname = {name};");
    let _ = writeln!(out, "\t\t}};");
}

fn write_target_build_config(
    out: &mut String,
    cfg_id: &str,
    name: &str,
    bundle_id: &str,
    _debug: bool,
) {
    let _ = writeln!(out, "\t\t{cfg_id} /* {name} */ = {{");
    let _ = writeln!(out, "\t\t\tisa = XCBuildConfiguration;");
    let _ = writeln!(out, "\t\t\tbuildSettings = {{");
    let _ = writeln!(out, "\t\t\t\tCODE_SIGN_IDENTITY = \"-\";");
    let _ = writeln!(out, "\t\t\t\tCODE_SIGN_STYLE = Automatic;");
    let _ = writeln!(out, "\t\t\t\tCURRENT_PROJECT_VERSION = 1;");
    let _ = writeln!(out, "\t\t\t\tDEVELOPMENT_TEAM = \"\";");
    let _ = writeln!(out, "\t\t\t\tENABLE_HARDENED_RUNTIME = NO;");
    let _ = writeln!(out, "\t\t\t\tGENERATE_INFOPLIST_FILE = YES;");
    let _ = writeln!(
        out,
        "\t\t\t\tINFOPLIST_FILE = \"Generated/Resources/Info.plist\";"
    );
    let _ = writeln!(
        out,
        "\t\t\t\tINFOPLIST_KEY_LSApplicationCategoryType = \"public.app-category.entertainment\";"
    );
    let _ = writeln!(out, "\t\t\t\tINFOPLIST_KEY_NSHighResolutionCapable = YES;");
    let _ = writeln!(
        out,
        "\t\t\t\tINFOPLIST_KEY_UIApplicationSceneManifest_Generation = YES;"
    );
    let _ = writeln!(
        out,
        "\t\t\t\tINFOPLIST_KEY_UIApplicationSupportsIndirectInputEvents = YES;"
    );
    let _ = writeln!(
        out,
        "\t\t\t\tINFOPLIST_KEY_UILaunchScreen_Generation = YES;"
    );
    let _ = writeln!(
        out,
        "\t\t\t\tINFOPLIST_KEY_UISupportedInterfaceOrientations_iPad = \"UIInterfaceOrientationPortrait UIInterfaceOrientationPortraitUpsideDown UIInterfaceOrientationLandscapeLeft UIInterfaceOrientationLandscapeRight\";"
    );
    let _ = writeln!(
        out,
        "\t\t\t\tINFOPLIST_KEY_UISupportedInterfaceOrientations_iPhone = \"UIInterfaceOrientationPortrait UIInterfaceOrientationLandscapeLeft UIInterfaceOrientationLandscapeRight\";"
    );
    let _ = writeln!(out, "\t\t\t\tIPHONEOS_DEPLOYMENT_TARGET = 17.0;");
    let _ = writeln!(
        out,
        "\t\t\t\tLD_RUNPATH_SEARCH_PATHS = \"@executable_path/Frameworks\";"
    );
    let _ = writeln!(
        out,
        "\t\t\t\t\"LD_RUNPATH_SEARCH_PATHS[sdk=macosx*]\" = \"@executable_path/../Frameworks\";"
    );
    let _ = writeln!(out, "\t\t\t\tMACOSX_DEPLOYMENT_TARGET = 15.0;");
    let _ = writeln!(out, "\t\t\t\tMARKETING_VERSION = 1.0;");
    let _ = writeln!(
        out,
        "\t\t\t\tPRODUCT_BUNDLE_IDENTIFIER = {};",
        pbx_str(bundle_id)
    );
    let _ = writeln!(out, "\t\t\t\tPRODUCT_NAME = \"$(TARGET_NAME)\";");
    let _ = writeln!(out, "\t\t\t\tREGISTER_APP_GROUPS = NO;");
    let _ = writeln!(
        out,
        "\t\t\t\tSUPPORTED_PLATFORMS = \"macosx iphoneos iphonesimulator\";"
    );
    let _ = writeln!(out, "\t\t\t\tSUPPORTS_MACCATALYST = NO;");
    let _ = writeln!(out, "\t\t\t\tSWIFT_EMIT_LOC_STRINGS = YES;");
    let _ = writeln!(out, "\t\t\t\tSWIFT_STRICT_CONCURRENCY = complete;");
    let _ = writeln!(out, "\t\t\t\tSWIFT_VERSION = 6.0;");
    let _ = writeln!(out, "\t\t\t\tTARGETED_DEVICE_FAMILY = \"1,2\";");
    let _ = writeln!(out, "\t\t\t}};");
    let _ = writeln!(out, "\t\t\tname = {name};");
    let _ = writeln!(out, "\t\t}};");
}

pub(super) fn generate_xcscheme(target_name: &str) -> String {
    let target_id = pbx_object_id(&format!("target:{target_name}"));
    let escaped_name = xml_escape(target_name);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Scheme
   LastUpgradeVersion = "1600"
   version = "1.7">
   <BuildAction
      parallelizeBuildables = "YES"
      buildImplicitDependencies = "YES">
      <BuildActionEntries>
         <BuildActionEntry
            buildForTesting = "YES"
            buildForRunning = "YES"
            buildForProfiling = "YES"
            buildForArchiving = "YES"
            buildForAnalyzing = "YES">
            <BuildableReference
               BuildableIdentifier = "primary"
               BlueprintIdentifier = "{target_id}"
               BuildableName = "{escaped_name}.app"
               BlueprintName = "{escaped_name}"
               ReferencedContainer = "container:{escaped_name}.xcodeproj">
            </BuildableReference>
         </BuildActionEntry>
      </BuildActionEntries>
   </BuildAction>
   <TestAction
      buildConfiguration = "Debug"
      selectedDebuggerIdentifier = "Xcode.DebuggerFoundation.Debugger.LLDB"
      selectedLauncherIdentifier = "Xcode.DebuggerFoundation.Launcher.LLDB"
      shouldUseLaunchSchemeArgsEnv = "YES">
      <Testables>
      </Testables>
   </TestAction>
   <LaunchAction
      buildConfiguration = "Debug"
      selectedDebuggerIdentifier = "Xcode.DebuggerFoundation.Debugger.LLDB"
      selectedLauncherIdentifier = "Xcode.DebuggerFoundation.Launcher.LLDB"
      launchStyle = "0"
      useCustomWorkingDirectory = "NO"
      ignoresPersistentStateOnLaunch = "NO"
      debugDocumentVersioning = "YES"
      debugServiceExtension = "internal"
      allowLocationSimulation = "YES">
      <BuildableProductRunnable
         runnableDebuggingMode = "0">
         <BuildableReference
            BuildableIdentifier = "primary"
            BlueprintIdentifier = "{target_id}"
            BuildableName = "{escaped_name}.app"
            BlueprintName = "{escaped_name}"
            ReferencedContainer = "container:{escaped_name}.xcodeproj">
         </BuildableReference>
      </BuildableProductRunnable>
   </LaunchAction>
   <ProfileAction
      buildConfiguration = "Release"
      shouldUseLaunchSchemeArgsEnv = "YES"
      savedToolIdentifier = ""
      useCustomWorkingDirectory = "NO"
      debugDocumentVersioning = "YES">
      <BuildableProductRunnable
         runnableDebuggingMode = "0">
         <BuildableReference
            BuildableIdentifier = "primary"
            BlueprintIdentifier = "{target_id}"
            BuildableName = "{escaped_name}.app"
            BlueprintName = "{escaped_name}"
            ReferencedContainer = "container:{escaped_name}.xcodeproj">
         </BuildableReference>
      </BuildableProductRunnable>
   </ProfileAction>
   <AnalyzeAction
      buildConfiguration = "Debug">
   </AnalyzeAction>
   <ArchiveAction
      buildConfiguration = "Release"
      revealArchiveInOrganizer = "YES">
   </ArchiveAction>
</Scheme>
"#
    )
}
