use super::*;

impl Parser<'_> {
    pub(super) fn audit_filters(&mut self, filters_node: Node<'_, '_>, file: &str, location: &str) {
        self.audit(
            filters_node,
            file,
            &[],
            &[
                "DropShadowFilter",
                "GlowFilter",
                "BevelFilter",
                "BlurFilter",
                "GradientGlowFilter",
                "GradientBevelFilter",
                "AdjustColorFilter",
            ],
        );
        let mut saw_filter = false;
        for f in elements(filters_node) {
            saw_filter = true;
            match f.tag_name().name() {
                "DropShadowFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "color",
                            "alpha",
                            "distance",
                            "strength",
                            "quality",
                            "inner",
                            "knockout",
                            "hideObject",
                            "isEnabled",
                        ],
                        &[],
                    );
                }
                "GlowFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "blurX",
                            "blurY",
                            "color",
                            "alpha",
                            "strength",
                            "quality",
                            "inner",
                            "knockout",
                            "isEnabled",
                        ],
                        &[],
                    );
                }
                "BevelFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "distance",
                            "highlightColor",
                            "highlightAlpha",
                            "shadowColor",
                            "shadowAlpha",
                            "strength",
                            "quality",
                            "type",
                            "knockout",
                            "isEnabled",
                        ],
                        &[],
                    );
                }
                "BlurFilter" => {
                    self.audit(f, file, &["blurX", "blurY", "quality", "isEnabled"], &[]);
                }
                "GradientGlowFilter" | "GradientBevelFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "distance",
                            "strength",
                            "quality",
                            "type",
                            "knockout",
                            "isEnabled",
                        ],
                        &["GradientEntry"],
                    );
                    for entry in elements(f).filter(|n| n.has_tag_name("GradientEntry")) {
                        self.audit(entry, file, &["color", "alpha", "ratio"], &[]);
                    }
                }
                "AdjustColorFilter" => {
                    self.audit(
                        f,
                        file,
                        &["brightness", "contrast", "saturation", "hue", "isEnabled"],
                        &[],
                    );
                }
                _ => {}
            }
        }
        if saw_filter {
            self.report.warning(
                "xfl_filters_approximated",
                location,
                "XFL display filters (DropShadow/Glow/Bevel/Blur/AdjustColor) are not rendered on authored elements yet.",
            );
        }
    }

    pub(super) fn parse_instance_filters(
        &mut self,
        filters_node: Node<'_, '_>,
        file: &str,
        location: &str,
    ) -> Vec<FilterSpec> {
        self.audit(
            filters_node,
            file,
            &[],
            &[
                "DropShadowFilter",
                "GlowFilter",
                "BevelFilter",
                "BlurFilter",
                "GradientGlowFilter",
                "GradientBevelFilter",
                "AdjustColorFilter",
            ],
        );
        let mut parsed = Vec::new();
        let mut omitted = false;
        for f in elements(filters_node) {
            let f_loc = self.location(f, file);
            let spec = match f.tag_name().name() {
                "DropShadowFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "color",
                            "alpha",
                            "distance",
                            "strength",
                            "quality",
                            "inner",
                            "knockout",
                            "hideObject",
                            "isEnabled",
                        ],
                        &[],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let distance = self.number(f, "distance", 4.0, file).clamp(-255.0, 255.0);
                    let angle = self.number(f, "angle", 45.0, file).clamp(-360.0, 360.0);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let alpha = self.number(f, "alpha", 1.0, file).clamp(0.0, 1.0);
                    let rgba = self.color(f.attribute("color").unwrap_or("#000000"), alpha, &f_loc);
                    let color =
                        (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
                    let strength = self.filter_strength(f, file);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    let inner = self.boolean(f, "inner", false, file);
                    let knockout = self.boolean(f, "knockout", false, file);
                    let hide_object = self.boolean(f, "hideObject", false, file);
                    Some(
                        FilterSpec::DropShadow {
                            distance,
                            angle,
                            color,
                            alpha,
                            blur_x,
                            blur_y,
                            strength,
                            quality,
                            inner,
                            knockout,
                            hide_object,
                        }
                        .with_enabled(enabled),
                    )
                }
                "GlowFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "blurX",
                            "blurY",
                            "color",
                            "alpha",
                            "strength",
                            "quality",
                            "inner",
                            "knockout",
                            "isEnabled",
                        ],
                        &[],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let alpha = self.number(f, "alpha", 1.0, file).clamp(0.0, 1.0);
                    let rgba = self.color(f.attribute("color").unwrap_or("#FF0000"), alpha, &f_loc);
                    let color =
                        (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
                    let strength = self.filter_strength(f, file);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    let inner = self.boolean(f, "inner", false, file);
                    let knockout = self.boolean(f, "knockout", false, file);
                    Some(
                        FilterSpec::Glow {
                            color,
                            alpha,
                            blur_x,
                            blur_y,
                            strength,
                            quality,
                            inner,
                            knockout,
                        }
                        .with_enabled(enabled),
                    )
                }
                "BevelFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "distance",
                            "highlightColor",
                            "highlightAlpha",
                            "shadowColor",
                            "shadowAlpha",
                            "strength",
                            "quality",
                            "type",
                            "knockout",
                            "isEnabled",
                        ],
                        &[],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let distance = self.number(f, "distance", 4.0, file).clamp(-255.0, 255.0);
                    let angle = self.number(f, "angle", 45.0, file).clamp(-360.0, 360.0);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let highlight_alpha =
                        self.number(f, "highlightAlpha", 1.0, file).clamp(0.0, 1.0);
                    let h_rgba = self.color(
                        f.attribute("highlightColor").unwrap_or("#FFFFFF"),
                        highlight_alpha,
                        &f_loc,
                    );
                    let highlight_color = (u32::from(h_rgba[0]) << 16)
                        | (u32::from(h_rgba[1]) << 8)
                        | u32::from(h_rgba[2]);
                    let shadow_alpha = self.number(f, "shadowAlpha", 1.0, file).clamp(0.0, 1.0);
                    let s_rgba = self.color(
                        f.attribute("shadowColor").unwrap_or("#000000"),
                        shadow_alpha,
                        &f_loc,
                    );
                    let shadow_color = (u32::from(s_rgba[0]) << 16)
                        | (u32::from(s_rgba[1]) << 8)
                        | u32::from(s_rgba[2]);
                    let strength = self.filter_strength(f, file);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    let placement = match f.attribute("type").unwrap_or("inner") {
                        "outer" => "outer".to_owned(),
                        "full" => "full".to_owned(),
                        _ => "inner".to_owned(),
                    };
                    let knockout = self.boolean(f, "knockout", false, file);
                    Some(
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
                            quality,
                            placement,
                            knockout,
                        }
                        .with_enabled(enabled),
                    )
                }
                "BlurFilter" => {
                    self.audit(f, file, &["blurX", "blurY", "quality", "isEnabled"], &[]);
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    Some(
                        FilterSpec::Blur {
                            blur_x,
                            blur_y,
                            quality,
                        }
                        .with_enabled(enabled),
                    )
                }
                "GradientGlowFilter" | "GradientBevelFilter" => {
                    self.audit(
                        f,
                        file,
                        &[
                            "angle",
                            "blurX",
                            "blurY",
                            "distance",
                            "strength",
                            "quality",
                            "type",
                            "knockout",
                            "isEnabled",
                        ],
                        &["GradientEntry"],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let distance = self.number(f, "distance", 4.0, file).clamp(-255.0, 255.0);
                    let angle = self.number(f, "angle", 45.0, file).clamp(-360.0, 360.0);
                    let blur_x = self.number(f, "blurX", 4.0, file).clamp(0.0, 255.0);
                    let blur_y = self.number(f, "blurY", 4.0, file).clamp(0.0, 255.0);
                    let strength = self.filter_strength(f, file);
                    let quality = self.integer(f, "quality", 1, file).clamp(1, 15) as u8;
                    let placement = match f.attribute("type").unwrap_or("inner") {
                        "outer" => "outer".to_owned(),
                        "full" => "full".to_owned(),
                        _ => "inner".to_owned(),
                    };
                    let knockout = self.boolean(f, "knockout", false, file);
                    let mut stops: Vec<(u32, f64, u8)> = Vec::new();
                    for entry in elements(f).filter(|n| n.has_tag_name("GradientEntry")) {
                        self.audit(entry, file, &["color", "alpha", "ratio"], &[]);
                        let e_loc = self.location(entry, file);
                        let alpha = self.number(entry, "alpha", 1.0, file).clamp(0.0, 1.0);
                        let rgba = self.color(
                            entry.attribute("color").unwrap_or("#000000"),
                            alpha,
                            &e_loc,
                        );
                        let color = (u32::from(rgba[0]) << 16)
                            | (u32::from(rgba[1]) << 8)
                            | u32::from(rgba[2]);
                        let raw_ratio = self.number(entry, "ratio", 0.0, file);
                        let ratio = if raw_ratio <= 1.0 {
                            (raw_ratio.clamp(0.0, 1.0) * 255.0).round() as u8
                        } else {
                            raw_ratio.clamp(0.0, 255.0).round() as u8
                        };
                        stops.push((color, alpha, ratio));
                    }
                    stops.sort_by_key(|s| s.2);
                    stops.truncate(16);
                    if stops.is_empty() {
                        stops.push((0x000000, 0.0, 0));
                        stops.push((0xFFFFFF, 1.0, 255));
                    }
                    let colors: Vec<u32> = stops.iter().map(|s| s.0).collect();
                    let alphas: Vec<f64> = stops.iter().map(|s| s.1).collect();
                    let ratios: Vec<u8> = stops.iter().map(|s| s.2).collect();
                    Some(
                        if f.tag_name().name() == "GradientGlowFilter" {
                            FilterSpec::GradientGlow {
                                distance,
                                angle,
                                colors,
                                alphas,
                                ratios,
                                blur_x,
                                blur_y,
                                strength,
                                quality,
                                placement,
                                knockout,
                            }
                        } else {
                            FilterSpec::GradientBevel {
                                distance,
                                angle,
                                colors,
                                alphas,
                                ratios,
                                blur_x,
                                blur_y,
                                strength,
                                quality,
                                placement,
                                knockout,
                            }
                        }
                        .with_enabled(enabled),
                    )
                }
                "AdjustColorFilter" => {
                    self.audit(
                        f,
                        file,
                        &["brightness", "contrast", "saturation", "hue", "isEnabled"],
                        &[],
                    );
                    let enabled = self.boolean(f, "isEnabled", true, file);
                    let brightness = self.number(f, "brightness", 0.0, file).clamp(-100.0, 100.0);
                    let contrast = self.number(f, "contrast", 0.0, file).clamp(-100.0, 100.0);
                    let saturation = self.number(f, "saturation", 0.0, file).clamp(-100.0, 100.0);
                    let hue = self.number(f, "hue", 0.0, file).clamp(-180.0, 180.0);
                    Some(
                        FilterSpec::ColorMatrix {
                            matrix: adjust_color_matrix(brightness, contrast, saturation, hue),
                        }
                        .with_enabled(enabled),
                    )
                }
                _ => None,
            };
            if let Some(filter) = spec {
                if parsed.len() < MAX_OBJECT_FILTERS {
                    parsed.push(filter);
                } else {
                    omitted = true;
                }
            } else {
                omitted = true;
            }
        }
        if omitted {
            self.report.warning(
                "xfl_filters_approximated",
                location,
                "Unsupported or excess (>8) XFL display filters were omitted.",
            );
        }
        parsed
    }
}
