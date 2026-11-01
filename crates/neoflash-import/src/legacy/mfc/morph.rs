//! Shape-tween morph records. Layout facts follow the pinned JPEXS reader
//! (0923d70870d11b236755af71337da8d0110a57aa, MorphRecordDecoder.java); this
//! is an independent bounded decoder. CPicMorphShape, CMorphSegment,
//! CMorphCurve and CMorphHintItem are not CPicObj subclasses: they have no
//! inherited object header and enter the shared MFC identity table through
//! counted pointer lists, whose null entries are retained as values.
use super::*;

impl Reader<'_> {
    /// MFC collection count: a word, or 0xffff followed by a dword.
    fn morph_count(&mut self) -> Result<usize> {
        let short = self.word()?;
        let count = if short == 0xffff {
            self.dword()? as usize
        } else {
            usize::from(short)
        };
        // Every pointer occupies at least two bytes.
        if count > MAX_RECORDS || count > (self.data.len() - self.pos) / 2 {
            return stop(
                "fla_record_limit",
                "Morph collection exceeds 50000 entries or the remaining input.",
            );
        }
        Ok(count)
    }
    fn morph_pointers(
        &mut self,
        id: u32,
        depth: usize,
        relation: TimelineRelation,
        out: &mut Vec<Option<u32>>,
    ) -> Result<()> {
        let count = self.morph_count()?;
        for _ in 0..count {
            let target = self.object(depth + 1, Some(id), relation)?;
            out.push(target);
        }
        Ok(())
    }
    fn point(&mut self) -> Result<[i32; 2]> {
        self.signed_words()
    }
    pub(super) fn morph_shape(&mut self, id: u32, depth: usize) -> Result<()> {
        let mut morph = TimelineMorphShape::default();
        let parsed = (|| {
            let revision = self.revision(2, "CPicMorphShape")?;
            morph.revision = Some(revision);
            morph.matrix_words_start = Some(self.signed_words()?);
            morph.matrix_words_end = Some(self.signed_words()?);
            if revision >= 2 {
                morph.extension_value = Some(self.dword()?);
                morph.acceleration = Some(self.dword()? as i32);
            }
            self.morph_pointers(
                id,
                depth,
                TimelineRelation::MorphSegment,
                &mut morph.segments,
            )?;
            self.morph_pointers(id, depth, TimelineRelation::MorphHint, &mut morph.hints)?;
            match geometry::decode_morph_styles(&self.data[self.pos..]) {
                Ok((fills, strokes, consumed)) => {
                    let items = fills.len() + strokes.len();
                    if self.retain_geometry {
                        if items > *self.retained_items_remaining {
                            return stop(
                                "fla_geometry_document_limit",
                                "Retained geometry exceeds 4000000 items across this document.",
                            );
                        }
                        *self.retained_items_remaining -= items;
                    }
                    self.pos += consumed;
                    morph.fills = fills;
                    morph.strokes = strokes;
                    Ok(())
                }
                Err(e) => {
                    self.pos += e.offset;
                    Err(Stop {
                        code: e.code,
                        message: e.message,
                    })
                }
            }
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Morph(Box::new(morph)));
        parsed
    }
    pub(super) fn morph_segment(&mut self, id: u32, depth: usize) -> Result<()> {
        let mut segment = TimelineMorphSegment::default();
        let parsed = (|| {
            segment.stroke_index_1 = Some(self.dword()? as i32);
            segment.stroke_index_2 = Some(self.dword()? as i32);
            segment.fill_index_1 = Some(self.dword()? as i32);
            segment.fill_index_2 = Some(self.dword()? as i32);
            segment.start_point_a = Some(self.point()?);
            segment.start_point_b = Some(self.point()?);
            self.morph_pointers(id, depth, TimelineRelation::MorphCurve, &mut segment.curves)
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::MorphSegment(segment));
        parsed
    }
    pub(super) fn morph_curve(&mut self, id: u32) -> Result<()> {
        let mut curve = TimelineMorphCurve::default();
        let parsed = (|| {
            curve.control_point_a = Some(self.point()?);
            curve.anchor_point_a = Some(self.point()?);
            curve.control_point_b = Some(self.point()?);
            curve.anchor_point_b = Some(self.point()?);
            curve.line_flags = Some(self.dword()?);
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::MorphCurve(curve));
        parsed
    }
    pub(super) fn morph_hint(&mut self, id: u32) -> Result<()> {
        let mut hint = TimelineMorphHint::default();
        let parsed = (|| {
            hint.point_a = Some(self.point()?);
            hint.point_b = Some(self.point()?);
            hint.index = Some(self.dword()?);
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::MorphHint(hint));
        parsed
    }
}
