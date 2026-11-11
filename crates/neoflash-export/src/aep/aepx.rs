use super::*;

// ============================================================================
// 2. After Effects XML Project (`.aepx`) Writer
// ============================================================================

fn xml_esc(s: &str) -> String {
    let filtered: String = s
        .chars()
        .filter(|&c| !c.is_control() || c == '\n' || c == '\r' || c == '\t')
        .collect();
    crate::xml_escape(&filtered)
}

pub(super) fn render_aepx_xml(model: &AeProjectModel, rifx_bytes: &[u8]) -> String {
    let mut xml = String::with_capacity(16_384);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        xml,
        "<AfterEffectsProject xmlns=\"http://ns.adobe.com/aep/1.0/\" version=\"24.0\" name=\"{}\" mainCompId=\"{}\">",
        xml_esc(&model.name),
        xml_esc(&model.main_comp_id)
    );

    xml.push_str("  <FootageItems>\n");
    for ft in &model.footage {
        let _ = writeln!(
            xml,
            "    <Footage id=\"{}\" name=\"{}\" path=\"{}\" mediaType=\"{}\"/>",
            xml_esc(&ft.id),
            xml_esc(&ft.name),
            xml_esc(&ft.relative_path),
            xml_esc(&ft.media_type)
        );
    }
    xml.push_str("  </FootageItems>\n");

    xml.push_str("  <Compositions>\n");
    for comp in &model.compositions {
        let _ = writeln!(
            xml,
            "    <Composition id=\"{}\" name=\"{}\" folder=\"{}\" width=\"{}\" height=\"{}\" fps=\"{:.4}\" durationFrames=\"{}\" bgColor=\"#{:02x}{:02x}{:02x}\">",
            xml_esc(&comp.id),
            xml_esc(&comp.name),
            xml_esc(&comp.folder),
            comp.width,
            comp.height,
            comp.fps,
            comp.duration_frames,
            (comp.background[0] * 255.0).round() as u8,
            (comp.background[1] * 255.0).round() as u8,
            (comp.background[2] * 255.0).round() as u8,
        );
        for m in &comp.markers {
            let _ = writeln!(
                xml,
                "      <Marker frame=\"{}\" duration=\"{}\" label=\"{}\">{}</Marker>",
                m.frame,
                m.duration_frames,
                xml_esc(&m.label),
                xml_esc(&m.comment)
            );
        }
        // Top-to-bottom AE stacking
        for layer in comp.layers.iter().rev() {
            let kind_tag = match &layer.kind {
                AeLayerKind::Shape { .. } => "ShapeLayer",
                AeLayerKind::Text { .. } => "TextLayer",
                AeLayerKind::PreComp { .. } => "PreCompLayer",
                AeLayerKind::Footage { .. } => "FootageLayer",
                AeLayerKind::Null => "NullLayer",
                AeLayerKind::Camera { .. } => "CameraLayer",
            };
            let _ = writeln!(
                xml,
                "      <{kind_tag} id=\"{}\" name=\"{}\" inFrame=\"{}\" outFrame=\"{}\" blendMode=\"{}\" threeD=\"{}\">",
                xml_esc(&layer.id),
                xml_esc(&layer.name),
                layer.in_frame,
                layer.out_frame,
                layer.blend_mode,
                layer.three_d
            );
            for (prop, expr_opt) in [
                ("Position", &layer.expressions.position),
                ("Scale", &layer.expressions.scale),
                ("Rotation", &layer.expressions.rotation),
                ("Opacity", &layer.expressions.opacity),
                ("Skew", &layer.expressions.skew),
                ("TimeRemap", &layer.expressions.time_remap),
                ("SourceText", &layer.expressions.source_text),
            ] {
                if let Some(expr) = expr_opt {
                    let _ = writeln!(
                        xml,
                        "        <Expression property=\"{prop}\">{}</Expression>",
                        xml_esc(expr)
                    );
                }
            }
            if let AeLayerKind::Shape { groups } = &layer.kind {
                for grp in groups {
                    let _ = writeln!(xml, "        <VectorGroup name=\"{}\">", xml_esc(&grp.name));
                    for track in &grp.contour_tracks {
                        let _ =
                            writeln!(xml, "          <BezierPath keyframes=\"{}\">", track.len());
                        for key in track {
                            let verts_str: Vec<String> = key
                                .value
                                .vertices
                                .iter()
                                .enumerate()
                                .map(|(vi, v)| {
                                    let ti = key
                                        .value
                                        .in_tangents
                                        .get(vi)
                                        .copied()
                                        .unwrap_or([0.0, 0.0]);
                                    let to = key
                                        .value
                                        .out_tangents
                                        .get(vi)
                                        .copied()
                                        .unwrap_or([0.0, 0.0]);
                                    format!(
                                        "{:.3},{:.3}({:.3},{:.3}|{:.3},{:.3})",
                                        finite_or(v[0], 0.0),
                                        finite_or(v[1], 0.0),
                                        finite_or(ti[0], 0.0),
                                        finite_or(ti[1], 0.0),
                                        finite_or(to[0], 0.0),
                                        finite_or(to[1], 0.0)
                                    )
                                })
                                .collect();
                            let _ = writeln!(
                                xml,
                                "            <Key frame=\"{}\" closed=\"{}\">{}</Key>",
                                key.frame,
                                key.value.closed,
                                verts_str.join(" ")
                            );
                        }
                        xml.push_str("          </BezierPath>\n");
                    }
                    xml.push_str("        </VectorGroup>\n");
                }
            }
            let _ = writeln!(xml, "      </{kind_tag}>");
        }
        xml.push_str("    </Composition>\n");
    }
    xml.push_str("  </Compositions>\n");

    let _ = writeln!(
        xml,
        "  <RifxPayload encoding=\"base64\">{}</RifxPayload>",
        STANDARD.encode(rifx_bytes)
    );
    xml.push_str("</AfterEffectsProject>\n");
    xml
}
