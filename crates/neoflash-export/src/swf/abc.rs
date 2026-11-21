//! AVM2 DoABC2 binary serializer, SymbolClass/ExportAssets/Scene tags, filters, and PlaceObject2/3.

use super::*;

fn order_and_resolve_abc_classes(classes: &[AbcClassSpec]) -> Vec<AbcClassSpec> {
    let mut resolved = classes.to_vec();
    let known_classes: Vec<(String, String)> = resolved
        .iter()
        .map(|c| (c.pkg.clone(), c.class_name.clone()))
        .collect();
    for cls in &mut resolved {
        for (kpkg, kname) in &known_classes {
            if kname == &cls.class_name && kpkg == &cls.pkg {
                continue;
            }
            if (!cls.pkg.is_empty() && kpkg == &cls.pkg)
                || cls.wildcard_packages.iter().any(|wp| wp == kpkg)
            {
                cls.imported_classes
                    .entry(kname.clone())
                    .or_insert_with(|| kpkg.clone());
            }
        }
        let spkg = cls.super_pkg.clone();
        let sname = cls.super_name.clone();
        let exact_exists = known_classes.iter().any(|(p, n)| p == &spkg && n == &sname);
        if !exact_exists {
            if let Some(imp_pkg) = cls.imported_classes.get(&sname)
                && known_classes
                    .iter()
                    .any(|(p, n)| p == imp_pkg && n == &sname)
            {
                cls.super_pkg = imp_pkg.clone();
            } else {
                let matches: Vec<String> = known_classes
                    .iter()
                    .filter(|(_, n)| n == &sname)
                    .map(|(p, _)| p.clone())
                    .collect();
                if matches.len() == 1 {
                    cls.super_pkg = matches[0].clone();
                }
            }
        }
    }

    let mut ordered = Vec::with_capacity(resolved.len());
    let mut visited = vec![false; resolved.len()];
    let mut visiting = vec![false; resolved.len()];

    fn visit(
        idx: usize,
        resolved: &[AbcClassSpec],
        visited: &mut [bool],
        visiting: &mut [bool],
        ordered: &mut Vec<AbcClassSpec>,
    ) {
        if visited[idx] {
            return;
        }
        if visiting[idx] {
            visited[idx] = true;
            ordered.push(resolved[idx].clone());
            return;
        }
        visiting[idx] = true;
        let spkg = &resolved[idx].super_pkg;
        let sname = &resolved[idx].super_name;
        if let Some(parent_idx) = resolved
            .iter()
            .position(|c| &c.pkg == spkg && &c.class_name == sname)
            && parent_idx != idx
        {
            visit(parent_idx, resolved, visited, visiting, ordered);
        }
        visiting[idx] = false;
        if !visited[idx] {
            visited[idx] = true;
            ordered.push(resolved[idx].clone());
        }
    }

    for i in 0..resolved.len() {
        visit(i, &resolved, &mut visited, &mut visiting, &mut ordered);
    }
    ordered
}

fn is_avm2_inherited_builtin_member(name: &str) -> bool {
    matches!(
        name,
        "x" | "y"
            | "z"
            | "scaleX"
            | "scaleY"
            | "scaleZ"
            | "alpha"
            | "visible"
            | "rotation"
            | "rotationX"
            | "rotationY"
            | "rotationZ"
            | "name"
            | "width"
            | "height"
            | "mask"
            | "parent"
            | "root"
            | "stage"
            | "loaderInfo"
            | "graphics"
            | "currentFrame"
            | "currentFrameLabel"
            | "totalFrames"
            | "currentLabel"
            | "currentLabels"
            | "currentScene"
            | "scenes"
            | "framesLoaded"
            | "isPlaying"
            | "enabled"
            | "trackAsMenu"
            | "useHandCursor"
            | "buttonMode"
            | "mouseEnabled"
            | "mouseChildren"
            | "doubleClickEnabled"
            | "tabEnabled"
            | "tabChildren"
            | "tabIndex"
            | "focusRect"
            | "transform"
            | "filters"
            | "blendMode"
            | "cacheAsBitmap"
            | "opaqueBackground"
            | "scrollRect"
            | "scale9Grid"
            | "soundTransform"
            | "dropTarget"
            | "hitArea"
            | "numChildren"
            | "textSnapshot"
            | "accessibilityProperties"
            | "contextMenu"
            | "stop"
            | "play"
            | "nextFrame"
            | "prevFrame"
            | "gotoAndStop"
            | "gotoAndPlay"
            | "nextScene"
            | "prevScene"
            | "addFrameScript"
            | "addChild"
            | "addChildAt"
            | "removeChild"
            | "removeChildAt"
            | "removeChildren"
            | "getChildAt"
            | "getChildByName"
            | "getChildIndex"
            | "setChildIndex"
            | "swapChildren"
            | "swapChildrenAt"
            | "contains"
            | "areInaccessibleObjectsUnderPoint"
            | "getObjectsUnderPoint"
            | "getBounds"
            | "getRect"
            | "globalToLocal"
            | "localToGlobal"
            | "hitTestObject"
            | "hitTestPoint"
            | "startDrag"
            | "stopDrag"
            | "addEventListener"
            | "removeEventListener"
            | "dispatchEvent"
            | "hasEventListener"
            | "willTrigger"
            | "toString"
            | "valueOf"
            | "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "setPropertyIsEnumerable"
            | "constructor"
            | "prototype"
    )
}

pub(super) fn build_abc_bytes(classes: &[AbcClassSpec]) -> Result<Vec<u8>, ExportError> {
    let ordered_classes = order_and_resolve_abc_classes(classes);
    let mut pool = AbcPoolBuilder::default();
    let object_mn = pool.add_qname("", "Object");
    let add_frame_script_mn = pool.add_qname("", "addFrameScript");

    struct CompiledClass {
        class_mn: u32,
        super_mn: u32,
        ancestor_mns: Vec<u32>,
        cinit_method: u32,
        iinit_method: u32,
        instance_slots: Vec<u32>,
        trait_methods: Vec<(u32, u32)>,
        static_slots: Vec<u32>,
        static_methods: Vec<(u32, u32)>,
    }

    let mut state = Avm2CompilerState::default();
    let mut compiled_classes = Vec::with_capacity(ordered_classes.len());
    let empty_outer = HashMap::new();

    for cls in &ordered_classes {
        let class_mn = pool.add_qname(&cls.pkg, &cls.class_name);
        let super_mn = pool.add_qname(&cls.super_pkg, &cls.super_name);
        let ancestor_chain = resolve_abc_ancestor_chain(cls, &ordered_classes);
        let ancestor_mns: Vec<u32> = ancestor_chain
            .iter()
            .map(|(apkg, aname)| pool.add_qname(apkg, aname))
            .collect();
        let chain_len = ancestor_mns.len() as u32;
        let cinit_scope_depth = 2 + chain_len;
        let method_scope_depth = 3 + chain_len;
        let class_imports = Avm2ImportCtx {
            current_pkg: cls.pkg.clone(),
            imported_classes: cls.imported_classes.clone(),
            wildcard_packages: cls.wildcard_packages.clone(),
        };

        // 1. cinit + static slots & static methods
        let mut static_slots = Vec::with_capacity(cls.static_fields.len());
        let mut cinit_stmts = Vec::new();
        for (sf_name, sf_init) in &cls.static_fields {
            static_slots.push(pool.add_qname("", sf_name));
            if let Some(init_expr) = sf_init {
                cinit_stmts.push(SwfStmt::Assign {
                    target: SwfLValue::Member(
                        Box::new(SwfExpr::Ident("this".to_owned())),
                        sf_name.clone(),
                    ),
                    op: SwfAssignOp::Assign,
                    value: init_expr.clone(),
                });
            }
        }
        let cinit_method = state.alloc_method(Vec::new());
        compile_avm2_scoped_unit(
            &mut pool,
            &mut state,
            cinit_method,
            &[],
            &cinit_stmts,
            &empty_outer,
            cinit_scope_depth,
            false,
            &class_imports,
            None,
            |_, _| Ok(()),
        )
        .map_err(|err| {
            ExportError::Options(format!(
                "AVM2 static initializer on class '{}' failed to compile: {err}",
                cls.class_name
            ))
        })?;

        let mut static_methods = Vec::with_capacity(cls.static_methods.len());
        for (sm_name, sm_params, sm_stmts) in &cls.static_methods {
            let fn_mn = pool.add_qname("", sm_name);
            let param_name_indices: Vec<u32> =
                sm_params.iter().map(|p| pool.add_string(p)).collect();
            let sm_idx = state.alloc_method(param_name_indices);
            compile_avm2_scoped_unit(
                &mut pool,
                &mut state,
                sm_idx,
                sm_params,
                sm_stmts,
                &empty_outer,
                method_scope_depth,
                true,
                &class_imports,
                None,
                |_, _| Ok(()),
            )
            .map_err(|err| {
                ExportError::Options(format!(
                    "AVM2 static method '{}.{}' failed to compile: {err}",
                    cls.class_name, sm_name
                ))
            })?;
            static_methods.push((fn_mn, sm_idx));
        }

        // 2. Instance slots & methods (frame scripts + extra methods)
        // Parse frame scripts upfront so top-level `var` / `function` declarations on timeline frames
        // are hoisted as `Trait_Slot`s on the timeline class (matching Adobe Flash Pro's AS3 compiler
        // so closures resolve them via `findproperty`/`getlex` on `this` while keeping `MethodBodyInfo`
        // `trait_cnt == 0` on the frame method itself).
        let mut parsed_frame_scripts = Vec::with_capacity(cls.frame_scripts.len());
        let mut reserved_instance_names: HashSet<String> = cls
            .instance_fields
            .iter()
            .map(|(n, _)| n.clone())
            .chain(cls.extra_methods.iter().map(|(n, _, _)| n.clone()))
            .chain(cls.static_fields.iter().map(|(n, _)| n.clone()))
            .chain(cls.static_methods.iter().map(|(n, _, _)| n.clone()))
            .chain(
                cls.frame_scripts
                    .iter()
                    .map(|(f_idx, _)| format!("frame{}", f_idx + 1)),
            )
            .collect();
        for (apkg, aname) in &ancestor_chain {
            if let Some(anc) = ordered_classes
                .iter()
                .find(|c| &c.pkg == apkg && &c.class_name == aname)
            {
                for (n, _) in &anc.instance_fields {
                    reserved_instance_names.insert(n.clone());
                }
                for (n, _, _) in &anc.extra_methods {
                    reserved_instance_names.insert(n.clone());
                }
            }
        }
        let mut hoisted_frame_slots = Vec::new();
        for (frame_idx, script_src) in &cls.frame_scripts {
            let stmts = parse_swf_statements_strict(script_src, true).map_err(|err| {
                ExportError::Options(format!(
                    "AVM2 frame script on class '{}' frame {} failed to parse: {err}",
                    cls.class_name,
                    frame_idx + 1
                ))
            })?;
            let mut fs_vars = Vec::new();
            let mut fs_funcs = Vec::new();
            collect_direct_decls_in_stmts(&stmts, &mut fs_vars, &mut fs_funcs);
            for decl_name in fs_funcs.into_iter().chain(fs_vars) {
                if !is_avm2_inherited_builtin_member(&decl_name)
                    && reserved_instance_names.insert(decl_name.clone())
                {
                    hoisted_frame_slots.push(decl_name);
                }
            }
            parsed_frame_scripts.push((*frame_idx, script_src.as_str(), stmts));
        }

        let mut instance_slots: Vec<u32> = cls
            .instance_fields
            .iter()
            .map(|(fname, _)| pool.add_qname("", fname))
            .collect();
        for slot_name in &hoisted_frame_slots {
            instance_slots.push(pool.add_qname("", slot_name));
        }
        let mut trait_methods = Vec::new();
        let mut frame_bindings: Vec<(u32, u32)> = Vec::new();

        for (frame_idx, script_src, stmts) in parsed_frame_scripts {
            let fn_name = format!("frame{}", frame_idx + 1);
            let fn_mn = pool.add_qname("", &fn_name);
            let m_idx = state.alloc_method(Vec::new());
            let mut frame_imports = class_imports.clone();
            extract_script_imports(
                script_src,
                &mut frame_imports.imported_classes,
                &mut frame_imports.wildcard_packages,
            );
            for (kpkg, kname) in ordered_classes.iter().map(|c| (&c.pkg, &c.class_name)) {
                if frame_imports.wildcard_packages.iter().any(|wp| wp == kpkg) {
                    frame_imports
                        .imported_classes
                        .entry(kname.clone())
                        .or_insert_with(|| kpkg.clone());
                }
            }
            compile_avm2_scoped_unit(
                &mut pool,
                &mut state,
                m_idx,
                &[],
                &stmts,
                &empty_outer,
                method_scope_depth,
                false,
                &frame_imports,
                None,
                |_, _| Ok(()),
            )
            .map_err(|err| {
                ExportError::Options(format!(
                    "AVM2 frame script on class '{}' frame {} failed to compile: {err}",
                    cls.class_name,
                    frame_idx + 1
                ))
            })?;

            trait_methods.push((fn_mn, m_idx));
            frame_bindings.push((frame_idx, fn_mn));
        }

        for (m_name, m_params, m_stmts) in &cls.extra_methods {
            let fn_mn = pool.add_qname("", m_name);
            let param_name_indices: Vec<u32> =
                m_params.iter().map(|p| pool.add_string(p)).collect();
            let m_idx = state.alloc_method(param_name_indices);

            compile_avm2_scoped_unit(
                &mut pool,
                &mut state,
                m_idx,
                m_params,
                m_stmts,
                &empty_outer,
                method_scope_depth,
                true,
                &class_imports,
                None,
                |_, _| Ok(()),
            )
            .map_err(|err| {
                ExportError::Options(format!(
                    "AVM2 method '{}.{}' failed to compile: {err}",
                    cls.class_name, m_name
                ))
            })?;
            trait_methods.push((fn_mn, m_idx));
        }

        // 3. iinit (constructor)
        let ctor_param_indices: Vec<u32> =
            cls.ctor_params.iter().map(|p| pool.add_string(p)).collect();
        let iinit_method = state.alloc_method(ctor_param_indices);
        let effective_ctor_stmts = build_effective_ctor_stmts(cls);
        let has_explicit_super = cls.has_explicit_super;
        let post_super = has_explicit_super.then(|| Avm2PostSuperInit {
            frame_bindings: frame_bindings.clone(),
            add_frame_script_mn,
            field_stmts: build_instance_field_assigns(cls),
        });
        compile_avm2_scoped_unit(
            &mut pool,
            &mut state,
            iinit_method,
            &cls.ctor_params,
            &effective_ctor_stmts,
            &empty_outer,
            method_scope_depth,
            cls.has_explicit_ctor,
            &class_imports,
            post_super,
            |p, iinit_code| {
                if !has_explicit_super {
                    iinit_code.u8(0xD0); // getlocal_0
                    iinit_code.u8(0x49); // constructsuper
                    iinit_code.u32_v(0);
                    if !frame_bindings.is_empty() {
                        iinit_code.u8(0x5D); // findpropstrict
                        iinit_code.u32_v(add_frame_script_mn);
                        for (frame_idx, fn_mn) in &frame_bindings {
                            emit_avm2_int(
                                p,
                                iinit_code,
                                i32::try_from(*frame_idx)
                                    .expect("SWF preflight bounds AVM2 frame indexes"),
                            );
                            iinit_code.u8(0x60); // getlex
                            iinit_code.u32_v(*fn_mn);
                        }
                        iinit_code.u8(0x4F); // callpropvoid
                        iinit_code.u32_v(add_frame_script_mn);
                        iinit_code.u32_v((frame_bindings.len() * 2) as u32);
                    }
                }
                Ok(())
            },
        )
        .map_err(|err| {
            ExportError::Options(format!(
                "AVM2 constructor on class '{}' failed to compile: {err}",
                cls.class_name
            ))
        })?;

        compiled_classes.push(CompiledClass {
            class_mn,
            super_mn,
            ancestor_mns,
            cinit_method,
            iinit_method,
            instance_slots,
            trait_methods,
            static_slots,
            static_methods,
        });
    }

    // Script init method
    let sinit_method = state.alloc_method(Vec::new());
    let mut sinit_code = SwfBitWriter::new();
    sinit_code.u8(0xD0); // getlocal_0
    sinit_code.u8(0x30); // pushscope
    let mut max_chain_len = 0u32;
    for (c_idx, cls) in compiled_classes.iter().enumerate() {
        max_chain_len = max_chain_len.max(cls.ancestor_mns.len() as u32);
        sinit_code.u8(0x65); // getscopeobject 0
        sinit_code.u8(0x00);
        sinit_code.u8(0x60); // getlex Object
        sinit_code.u32_v(object_mn);
        sinit_code.u8(0x30); // pushscope
        for &anc_mn in &cls.ancestor_mns {
            sinit_code.u8(0x60); // getlex Ancestor
            sinit_code.u32_v(anc_mn);
            sinit_code.u8(0x30); // pushscope
        }
        let base_mn = cls.ancestor_mns.last().copied().unwrap_or(object_mn);
        sinit_code.u8(0x60); // getlex super
        sinit_code.u32_v(base_mn);
        sinit_code.u8(0x58); // newclass
        sinit_code.u32_v(c_idx as u32);
        for _ in 0..(1 + cls.ancestor_mns.len()) {
            sinit_code.u8(0x1D); // popscope
        }
        sinit_code.u8(0x68); // initproperty
        sinit_code.u32_v(cls.class_mn);
    }
    sinit_code.u8(0x47); // returnvoid
    state.bodies.push(AbcMethodBodySpec {
        method_idx: sinit_method,
        max_stack: 4,
        local_count: 1,
        init_scope_depth: 1,
        max_scope_depth: 3 + max_chain_len,
        code: sinit_code.buf,
        exceptions: Vec::new(),
        activation_traits: Vec::new(),
    });

    // Serialize ABC 46.16 stream
    let mut w = SwfBitWriter::new();
    w.u16(16); // minor_version = 16
    w.u16(46); // major_version = 46

    // Constant pool
    w.u32_v(if pool.ints.is_empty() {
        0
    } else {
        (pool.ints.len() + 1) as u32
    });
    for &v in &pool.ints {
        w.u32_v(v as u32);
    }
    w.u32_v(if pool.uints.is_empty() {
        0
    } else {
        (pool.uints.len() + 1) as u32
    });
    for &v in &pool.uints {
        w.u32_v(v);
    }
    w.u32_v(if pool.doubles.is_empty() {
        0
    } else {
        (pool.doubles.len() + 1) as u32
    });
    for &v in &pool.doubles {
        w.bytes(&v.to_le_bytes());
    }
    w.u32_v((pool.strings.len() + 1) as u32);
    for s in &pool.strings {
        w.u32_v(s.len() as u32);
        w.bytes(s.as_bytes());
    }
    w.u32_v((pool.namespaces.len() + 1) as u32);
    for &(kind, s_idx) in &pool.namespaces {
        w.u8(kind);
        w.u32_v(s_idx);
    }
    w.u32_v(if pool.ns_sets.is_empty() {
        0
    } else {
        (pool.ns_sets.len() + 1) as u32
    });
    for set in &pool.ns_sets {
        w.u32_v(set.len() as u32);
        for &ns_idx in set {
            w.u32_v(ns_idx);
        }
    }
    w.u32_v((pool.multinames.len() + 1) as u32);
    for &(kind, ns_idx, name_idx) in &pool.multinames {
        w.u8(kind);
        if kind == 0x1B || kind == 0x1C {
            w.u32_v(ns_idx);
        } else {
            w.u32_v(ns_idx);
            w.u32_v(name_idx);
        }
    }

    // Methods
    w.u32_v(state.methods.len() as u32);
    for m in &state.methods {
        let p_count = m.param_names.len() as u32;
        w.u32_v(p_count);
        w.u32_v(0); // return_type = *
        for _ in 0..p_count {
            w.u32_v(0); // param_type = *
        }
        w.u32_v(0); // name = ""
        let mut flags = 0u8;
        if m.needs_activation {
            flags |= 0x02; // NEED_ACTIVATION
        }
        if !m.param_names.is_empty() {
            flags |= 0x80; // HAS_PARAM_NAMES
        }
        w.u8(flags);
        if !m.param_names.is_empty() {
            for &pn_idx in &m.param_names {
                w.u32_v(pn_idx);
            }
        }
    }

    // Metadata
    w.u32_v(0);

    // Instances
    w.u32_v(compiled_classes.len() as u32);
    for cls in &compiled_classes {
        w.u32_v(cls.class_mn);
        w.u32_v(cls.super_mn);
        w.u8(0x00); // flags = dynamic (0)
        w.u32_v(0); // intrf_count = 0
        w.u32_v(cls.iinit_method);
        w.u32_v((cls.instance_slots.len() + cls.trait_methods.len()) as u32);
        for &slot_mn in &cls.instance_slots {
            w.u32_v(slot_mn);
            w.u8(0x00); // Trait_Slot
            w.u32_v(0); // auto slot_id = 0
            w.u32_v(0); // type_name = *
            w.u32_v(0); // vindex = 0
        }
        for &(m_mn, m_idx) in &cls.trait_methods {
            w.u32_v(m_mn);
            w.u8(0x01); // Trait_Method
            w.u32_v(0); // disp_id = 0
            w.u32_v(m_idx);
        }
    }

    // Classes
    for cls in &compiled_classes {
        w.u32_v(cls.cinit_method);
        w.u32_v((cls.static_slots.len() + cls.static_methods.len()) as u32);
        for &slot_mn in &cls.static_slots {
            w.u32_v(slot_mn);
            w.u8(0x00); // Trait_Slot
            w.u32_v(0); // auto slot_id = 0
            w.u32_v(0); // type_name = *
            w.u32_v(0); // vindex = 0
        }
        for &(m_mn, m_idx) in &cls.static_methods {
            w.u32_v(m_mn);
            w.u8(0x01); // Trait_Method
            w.u32_v(0); // disp_id = 0
            w.u32_v(m_idx);
        }
    }

    // Scripts (1 script containing all classes)
    w.u32_v(1);
    w.u32_v(sinit_method);
    w.u32_v(compiled_classes.len() as u32);
    for (c_idx, cls) in compiled_classes.iter().enumerate() {
        w.u32_v(cls.class_mn);
        w.u8(0x04); // Trait_Class
        w.u32_v((c_idx + 1) as u32); // slot_id
        w.u32_v(c_idx as u32); // class_idx
    }

    // Method bodies
    w.u32_v(state.bodies.len() as u32);
    for body in state.bodies {
        w.u32_v(body.method_idx);
        w.u32_v(body.max_stack);
        w.u32_v(body.local_count);
        w.u32_v(body.init_scope_depth);
        w.u32_v(body.max_scope_depth);
        w.u32_v(body.code.len() as u32);
        w.bytes(&body.code);
        w.u32_v(body.exceptions.len() as u32);
        for exc in body.exceptions {
            w.u32_v(exc.from);
            w.u32_v(exc.to);
            w.u32_v(exc.target);
            w.u32_v(exc.exc_type);
            w.u32_v(exc.var_name);
        }
        w.u32_v(body.activation_traits.len() as u32);
        for (qname_mn, slot_id) in body.activation_traits {
            w.u32_v(qname_mn);
            w.u8(0x00); // Trait_Slot
            w.u32_v(slot_id);
            w.u32_v(0); // type_name = *
            w.u32_v(0); // vindex = 0
        }
    }

    Ok(w.buf)
}

pub(super) fn build_do_abc2_payload(name: &str, abc_bytes: &[u8]) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u32(1); // lazyInitializeFlag = 1
    w.c_string(name);
    w.bytes(abc_bytes);
    w.buf
}

pub(super) fn build_symbol_class_payload(bindings: &[(u16, String)]) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(bindings.len() as u16);
    for (cid, cls_name) in bindings {
        w.u16(*cid);
        w.c_string(cls_name);
    }
    w.buf
}

pub(super) fn build_export_assets_payload(exports: &[(u16, String)]) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(exports.len() as u16);
    for (cid, name) in exports {
        w.u16(*cid);
        w.c_string(name);
    }
    w.buf
}

pub(super) fn build_define_scene_and_frame_label_data_payload(scenes: &[(u32, String)]) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u32_v(scenes.len() as u32);
    for (offset, name) in scenes {
        w.u32_v(*offset);
        w.c_string(name);
    }
    w.u32_v(0); // frame label count = 0 (emitted via FrameLabel tag 43)
    w.buf
}

pub(super) fn rgb_alpha_to_rgba(rgb: u32, alpha: f64) -> Color {
    let r = ((rgb >> 16) & 0xff) as u8;
    let g = ((rgb >> 8) & 0xff) as u8;
    let b = (rgb & 0xff) as u8;
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    [r, g, b, a]
}

pub(super) fn write_swf_filter_list(w: &mut SwfBitWriter, filters: &[FilterSpec]) {
    let active_filters: Vec<&FilterSpec> = filters
        .iter()
        .filter(|filter| filter.is_enabled())
        .take(16)
        .collect();
    let count = active_filters.len();
    w.u8(count as u8);
    let to_fixed16 = |v: f64| -> i32 { (v * 65536.0).round() as i32 };
    let to_fixed8 = |v: f64| -> i16 { (v.clamp(0.0, 127.0) * 256.0).round() as i16 };

    for filter in active_filters {
        let quality_level = filter.quality_level().clamp(1, 15);
        match filter {
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                inner,
                knockout,
                hide_object,
            } => {
                w.u8(0); // DropShadowFilter
                w.rgba(rgb_alpha_to_rgba(*color, *alpha));
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                w.u32(to_fixed16(angle.to_radians()) as u32);
                w.u32(to_fixed16(*distance) as u32);
                w.i16(to_fixed8(*strength));
                let composite_source = !*hide_object;
                let flags: u8 = (if *inner { 0x80 } else { 0 })
                    | (if *knockout { 0x40 } else { 0 })
                    | (if composite_source { 0x20 } else { 0 })
                    | (quality_level & 0x1F);
                w.u8(flags);
            }
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality: _,
            } => {
                w.u8(1); // BlurFilter
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                let flags: u8 = (quality_level & 0x1F) << 3;
                w.u8(flags);
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                inner,
                knockout,
            } => {
                w.u8(2); // GlowFilter
                w.rgba(rgb_alpha_to_rgba(*color, *alpha));
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                w.i16(to_fixed8(*strength));
                let flags: u8 = (if *inner { 0x80 } else { 0 })
                    | (if *knockout { 0x40 } else { 0 })
                    | 0x20
                    | (quality_level & 0x1F);
                w.u8(flags);
            }
            FilterSpec::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                placement,
                knockout,
            } => {
                w.u8(3); // BevelFilter
                w.rgba(rgb_alpha_to_rgba(*shadow_color, *shadow_alpha));
                w.rgba(rgb_alpha_to_rgba(*highlight_color, *highlight_alpha));
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                w.u32(to_fixed16(angle.to_radians()) as u32);
                w.u32(to_fixed16(*distance) as u32);
                w.i16(to_fixed8(*strength));
                let is_inner = placement == "inner";
                let on_top = placement == "full";
                let flags: u8 = (if is_inner { 0x80 } else { 0 })
                    | (if *knockout { 0x40 } else { 0 })
                    | 0x20
                    | (if on_top { 0x10 } else { 0 })
                    | (quality_level & 0x0F);
                w.u8(flags);
            }
            FilterSpec::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality: _,
                placement,
                knockout,
            }
            | FilterSpec::GradientBevel {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality: _,
                placement,
                knockout,
            } => {
                let is_grad_bevel = matches!(filter, FilterSpec::GradientBevel { .. });
                w.u8(if is_grad_bevel { 7 } else { 4 });
                let n = colors
                    .len()
                    .min(alphas.len())
                    .min(ratios.len())
                    .clamp(1, 16);
                w.u8(n as u8);
                for i in 0..n {
                    w.rgba(rgb_alpha_to_rgba(colors[i], alphas[i]));
                }
                for &ratio in ratios.iter().take(n) {
                    w.u8(ratio);
                }
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                w.u32(to_fixed16(angle.to_radians()) as u32);
                w.u32(to_fixed16(*distance) as u32);
                w.i16(to_fixed8(*strength));
                let is_inner = placement == "inner";
                let on_top = placement == "full";
                let flags: u8 = (if is_inner { 0x80 } else { 0 })
                    | (if *knockout { 0x40 } else { 0 })
                    | 0x20
                    | (if on_top { 0x10 } else { 0 })
                    | (quality_level & 0x0F);
                w.u8(flags);
            }
            FilterSpec::Convolution {
                matrix_x,
                matrix_y,
                matrix,
                divisor,
                bias,
                preserve_alpha,
                clamp,
                color,
                alpha,
            } => {
                w.u8(5); // ConvolutionFilter
                let mx = (*matrix_x).clamp(1, 15);
                let my = (*matrix_y).clamp(1, 15);
                w.u8(mx);
                w.u8(my);
                w.f32(*divisor as f32);
                w.f32(*bias as f32);
                let total = usize::from(mx) * usize::from(my);
                for i in 0..total {
                    w.f32(matrix.get(i).copied().unwrap_or(0.0) as f32);
                }
                w.rgba(rgb_alpha_to_rgba(*color, *alpha));
                let flags: u8 =
                    (if *clamp { 0x02 } else { 0 }) | (if *preserve_alpha { 0x01 } else { 0 });
                w.u8(flags);
            }
            FilterSpec::ColorMatrix { matrix } => {
                w.u8(6); // ColorMatrixFilter
                for &v in matrix {
                    w.f32(v as f32);
                }
            }
        }
    }
}

pub(super) fn encode_swf_blend_mode(blend: BlendMode) -> u8 {
    match blend {
        BlendMode::Normal => 1,
        BlendMode::Layer => 2,
        BlendMode::Multiply => 3,
        BlendMode::Screen => 4,
        BlendMode::Lighten => 5,
        BlendMode::Darken => 6,
        BlendMode::Difference => 7,
        BlendMode::Add => 8,
        BlendMode::Subtract => 9,
        BlendMode::Invert => 10,
        BlendMode::Alpha => 11,
        BlendMode::Erase => 12,
        BlendMode::Overlay => 13,
        BlendMode::Hardlight => 14,
    }
}

#[derive(Clone, Debug)]
pub(super) struct SlotPlacement {
    pub(super) character_id: u16,
    pub(super) matrix: [f64; 6],
    pub(super) color_transform: ColorTransform,
    pub(super) alpha: f64,
    pub(super) ratio: Option<u16>,
    pub(super) name: String,
    pub(super) clip_depth: Option<u16>,
    pub(super) visible: bool,
    pub(super) cache_as_bitmap: bool,
    pub(super) blend_mode: BlendMode,
    pub(super) filters: Vec<FilterSpec>,
    pub(super) clip_actions: Option<Vec<u8>>,
}

impl SlotPlacement {
    pub(super) fn same_state(&self, other: &Self) -> bool {
        self.character_id == other.character_id
            && self.matrix == other.matrix
            && self.color_transform == other.color_transform
            && (self.alpha - other.alpha).abs() <= 1e-6
            && self.ratio == other.ratio
            && self.name == other.name
            && self.clip_depth == other.clip_depth
            && self.visible == other.visible
            && self.cache_as_bitmap == other.cache_as_bitmap
            && self.blend_mode == other.blend_mode
            && self.filters == other.filters
            && self.clip_actions == other.clip_actions
    }
}

pub(super) fn write_place_object_tag(
    out_tags: &mut SwfBitWriter,
    depth: u16,
    slot: &SlotPlacement,
    prev: Option<&SlotPlacement>,
) {
    let is_move = prev.is_some();
    let prev_had_cxform =
        prev.is_some_and(|p| !p.color_transform.is_identity() || (p.alpha - 1.0).abs() > 1e-5);
    let prev_had_filters = prev.is_some_and(|p| p.filters.iter().any(FilterSpec::is_enabled));
    let prev_had_blend = prev.is_some_and(|p| p.blend_mode != BlendMode::Normal);
    let prev_had_cache = prev.is_some_and(|p| p.cache_as_bitmap);
    let prev_had_vis = prev.is_some_and(|p| !p.visible);

    let has_cxform =
        !slot.color_transform.is_identity() || (slot.alpha - 1.0).abs() > 1e-5 || prev_had_cxform;
    let has_ratio = slot.ratio.is_some();
    let has_name = !slot.name.is_empty() && !is_move;
    let has_clip_depth = slot.clip_depth.is_some() && !is_move;
    let has_clip_actions = slot.clip_actions.is_some() && !is_move;
    let has_filters = slot.filters.iter().any(FilterSpec::is_enabled) || prev_had_filters;
    let has_blend = slot.blend_mode != BlendMode::Normal || prev_had_blend;
    let has_cache = slot.cache_as_bitmap || prev_had_cache;
    let has_vis = !slot.visible || prev_had_vis;

    let use_place3 = has_filters || has_blend || has_cache || has_vis;
    let tag_code: u16 = if use_place3 { 70 } else { 26 };

    let flags1: u8 = (if has_clip_actions { 0x80 } else { 0 })
        | (if has_clip_depth { 0x40 } else { 0 })
        | (if has_name { 0x20 } else { 0 })
        | (if has_ratio { 0x10 } else { 0 })
        | (if has_cxform { 0x08 } else { 0 })
        | 0x04 // has_matrix
        | (if !is_move { 0x02 } else { 0 }) // has_character
        | (if is_move { 0x01 } else { 0 });

    let flags2: u8 = (if has_vis { 0x20 } else { 0 })
        | (if has_cache { 0x04 } else { 0 })
        | (if has_blend { 0x02 } else { 0 })
        | (if has_filters { 0x01 } else { 0 });

    let mut w = SwfBitWriter::new();
    w.u8(flags1);
    if use_place3 {
        w.u8(flags2);
    }
    w.u16(depth);
    if !is_move {
        w.u16(slot.character_id);
    }
    w.matrix_px(slot.matrix);
    if has_cxform {
        w.cxform_with_alpha(&slot.color_transform, slot.alpha);
    }
    if let Some(rat) = slot.ratio {
        w.u16(rat);
    }
    if has_name {
        w.c_string(&slot.name);
    }
    if let Some(cd) = slot.clip_depth
        && has_clip_depth
    {
        w.u16(cd);
    }
    if use_place3 {
        if has_filters {
            write_swf_filter_list(&mut w, &slot.filters);
        }
        if has_blend {
            w.u8(encode_swf_blend_mode(slot.blend_mode));
        }
        if has_cache {
            w.u8(u8::from(slot.cache_as_bitmap));
        }
        if has_vis {
            w.u8(u8::from(slot.visible));
        }
    }
    if let Some(clip_bytes) = &slot.clip_actions
        && has_clip_actions
    {
        w.bytes(clip_bytes);
    }
    out_tags.write_tag(tag_code, &w.buf);
}
