//! Exact decoding of the typed action-command containers written by Flash 2–5
//! and Flash MX, followed by an ActionScript 1 translation.
//!
//! These containers hold Actions-panel *authoring commands* (one record per
//! panel line, with typed operands), not SWF bytecode. Field layouts and the
//! command numbering are format facts documented by the pinned JPEXS FLA
//! reader (LGPL-2.1, 0923d70870d11b236755af71337da8d0110a57aa:
//! ActionRecordDecoder.java, CompiledActionRecordDecoder.java,
//! ExtendedCompiledActionRecordDecoder.java, FlaCommandKind.java). Property
//! indices and button key codes follow the SWF file format specification. This
//! Rust implementation is independently written; no code was copied.
//!
//! Decoding is sequential and bounded: every count is checked against the
//! remaining bytes and the shared item budgets, every string is a bounded
//! CString, and an unknown command kind/revision stops explicitly because its
//! length is not self-describing. Imported scripts are never executed.
use super::*;

#[cfg(test)]
mod tests;
mod translate;

/// Value of the archive's "no wait" sentinel in historical commands.
const NO_WAIT: i16 = -16000;
/// Smallest encoded historical command: revision, kind, three empty strings, frame.
const MIN_HISTORICAL_BYTES: usize = 7;

/// Command types whose compiled body is only a revision plus optional metadata.
fn simple_compiled(kind: u32) -> bool {
    matches!(
        kind,
        1 | 2
            | 3
            | 4
            | 9
            | 12
            | 14
            | 16
            | 18
            | 19
            | 21
            | 29
            | 38
            | 42
            | 45
            | 48
            | 51
            | 52
            | 54
            | 58
            | 61
            | 64
            | 66
            | 68
            | 70
            | 72
            | 74
            | 81
            | 85
    )
}

/// Operand layout of the remaining compiled command types, in archive order.
#[derive(Clone, Copy)]
enum Op {
    Str(&'static str),
    Word(&'static str),
    Param,
    WordFrom(u32, &'static str),
    ByteFrom(u32, &'static str),
}

/// Maximum command revision, metadata revision and operands for a compiled type.
fn compiled_layout(kind: u32, revision: u32) -> Option<(u32, u32, Vec<Op>)> {
    use Op::*;
    let three = matches!(kind, 39 | 41 | 44 | 47 | 59 | 78 | 80 | 83);
    let max = if kind == 37 {
        4
    } else if three {
        3
    } else {
        2
    };
    let meta = if kind == 37 {
        4
    } else if matches!(kind, 39 | 41 | 44 | 47 | 59 | 80) {
        3
    } else {
        2
    };
    let ops = match kind {
        6 | 49 | 24 => vec![Param, Param],
        7 => vec![
            Word("discarded_legacy_dword"),
            Param,
            Word("request_method"),
            Param,
            Word("target_mode"),
        ],
        8 | 22 | 27 | 30 | 36 => vec![Param],
        10 => vec![Str("scene"), Str("frame"), Word("frame_mode")],
        15 | 17 | 43 => vec![Str("condition")],
        20 => vec![Str("condition"), ByteFrom(2, "empty_body")],
        33 => vec![Str("include_path")],
        40 => vec![Str("return_value")],
        41 | 53 | 69 => vec![Str("expression")],
        50 => vec![Str("object")],
        57 => vec![Str("init_clip_index")],
        65 => vec![Str("exception")],
        75 => vec![Str("import_name")],
        84 => vec![Str("configuration")],
        23 => vec![Word("property_index"), Param, Param],
        25 => vec![Word("discarded_legacy_dword"), Param, Param, Str("depth")],
        26 => vec![
            Param,
            Str("left"),
            Str("top"),
            Str("right"),
            Str("bottom"),
            Word("lock_center"),
            Word("constrain"),
            Word("value3"),
        ],
        28 => vec![Str("comment")],
        31 => vec![Param, Word("target_mode")],
        32 => vec![Param, Word("request_method"), Param, Word("target_mode")],
        34 => vec![
            Word("discarded_legacy_dword"),
            Param,
            Word("target_mode"),
            Word("bounds_mode"),
            Word("print_as_bitmap"),
        ],
        37 => vec![
            Str("function_signature"),
            Str("parameters"),
            WordFrom(1, "function_flags"),
            ByteFrom(3, "declaration_only"),
        ],
        39 => vec![Str("declaration"), WordFrom(1, "variable_flags")],
        44 if revision == 0 => vec![Str("condition")],
        44 => vec![Str("initializer"), Str("condition"), Str("increment")],
        47 | 80 => vec![Str("iterator"), Str("collection")],
        73 => vec![Str("name"), Str("interfaces")],
        55 | 56 => vec![Str("case_expression"), Word("is_default_case")],
        59 => vec![
            Str("invocation_signature"),
            Str("object"),
            Str("arguments"),
            WordFrom(1, "object_expression_flags"),
            WordFrom(1, "value2"),
        ],
        60 => vec![Str("object"), Str("method"), Str("parameters")],
        62 | 79 => vec![Str("object"), Str("method"), Str("arguments")],
        63 | 67 | 77 => vec![],
        71 => vec![
            Str("name"),
            Str("super_class"),
            Str("interfaces"),
            Word("dynamic"),
            WordFrom(1, "intrinsic"),
        ],
        // 46/82 and 76/78/83 have revision-gated bodies handled by the caller.
        46 | 82 | 76 | 78 | 83 => vec![],
        _ => return None,
    };
    Some((max, meta, ops))
}

impl Reader<'_> {
    /// Decodes one action container body and stores its typed commands and
    /// translation in `actions`. Position advances exactly over the commands.
    pub(super) fn decode_actions(
        &mut self,
        actions: &mut TimelineActions,
        form: TimelineActionForm,
        list_revision: Option<u32>,
        count: u32,
    ) -> Result<()> {
        let mut decoded = TimelineDecodedActions {
            form,
            list_revision,
            commands: vec![],
            source: String::new(),
            untranslated: vec![],
            needs_review: false,
        };
        let min = match form {
            TimelineActionForm::Compiled => 4,
            _ => MIN_HISTORICAL_BYTES,
        };
        let count = count as usize;
        let result = if count > MAX_RECORDS || count > (self.data.len() - self.pos) / min {
            stop(
                "fla_record_limit",
                "Action command count exceeds the remaining input or item budget.",
            )
        } else {
            (|| {
                for _ in 0..count {
                    self.retain_budget()?;
                    match form {
                        TimelineActionForm::Compiled => self
                            .compiled_command(&mut decoded.commands, list_revision.unwrap_or(0))?,
                        _ => self.historical_command(&mut decoded.commands)?,
                    }
                }
                Ok(())
            })()
        };
        if result.is_ok() {
            if form == TimelineActionForm::HistoricalCommand
                && decoded.commands.first().is_some_and(|c| c.kind == 0)
            {
                actions.presence = TimelineActionPresence::Empty;
            }
            translate::translate(self.data, &mut decoded);
        }
        actions.decoded = Some(decoded);
        result
    }

    fn command_start(&self, commands: &mut Vec<TimelineActionCommand>) -> usize {
        commands.push(TimelineActionCommand {
            encoded: TimelineByteSpan {
                start: self.pos,
                end: self.pos,
            },
            complete: false,
            kind: 0,
            revision: None,
            body_present: true,
            fields: vec![],
        });
        commands.len() - 1
    }

    fn command_finish(
        &self,
        commands: &mut [TimelineActionCommand],
        index: usize,
        result: Result<()>,
    ) -> Result<()> {
        commands[index].encoded.end = self.pos;
        commands[index].complete = result.is_ok();
        result
    }

    fn historical_command(&mut self, commands: &mut Vec<TimelineActionCommand>) -> Result<()> {
        let index = self.command_start(commands);
        let result = (|| {
            let command = &mut commands[index];
            let revision = self.revision(10, "historical action")?;
            command.revision = Some(revision.into());
            command.kind = self.byte()?.into();
            let mut put = |name: &str, value| {
                command.fields.push(TimelineActionField {
                    name: name.into(),
                    value,
                })
            };
            for name in ["url", "window", "page"] {
                put(name, TimelineActionValue::String(self.string()?));
            }
            put("frame_number", TimelineActionValue::Word(self.word()?));
            if revision >= 1 {
                put(
                    "wait_for_extra_frames",
                    TimelineActionValue::Signed(self.word()? as i16 as i32),
                );
            }
            if revision >= 2 {
                put("label", TimelineActionValue::String(self.string()?));
            }
            if revision >= 3 {
                for name in ["arg0", "arg1", "target"] {
                    put(name, TimelineActionValue::String(self.string()?));
                }
                put("level", TimelineActionValue::Signed(self.dword()? as i32));
            }
            if revision >= 4 {
                for name in [
                    "release",
                    "press",
                    "release_outside",
                    "roll_over",
                    "roll_out",
                    "drag_over",
                    "drag_out",
                ] {
                    put(name, TimelineActionValue::Dword(self.dword()?));
                }
            }
            if revision >= 5 {
                put("property", TimelineActionValue::Dword(self.dword()?));
                for name in [
                    "reserved_string1",
                    "reserved_string2",
                    "depth",
                    "variable_name",
                ] {
                    put(name, TimelineActionValue::String(self.string()?));
                }
            }
            if revision >= 6 {
                put("method", TimelineActionValue::String(self.string()?));
                put("load_variables", TimelineActionValue::Dword(self.dword()?));
            }
            if revision >= 7 {
                for name in ["arg2", "arg3"] {
                    put(name, TimelineActionValue::String(self.string()?));
                }
                for name in [
                    "variable_name_is_expression",
                    "url_is_expression",
                    "arg0_is_expression",
                    "arg1_is_expression",
                    "arg2_is_expression",
                    "arg3_is_expression",
                    "window_is_expression",
                    "target_is_expression",
                    "depth_is_expression",
                    "start",
                    "constrain_to_rectangle",
                    "lock_mouse_to_center",
                ] {
                    put(name, TimelineActionValue::Dword(self.dword()?));
                }
            }
            if revision >= 8 {
                put("on_key_press", TimelineActionValue::Dword(self.dword()?));
                put("key", TimelineActionValue::Dword(self.dword()?));
            }
            if revision >= 9 {
                let length = self.dword()? as usize;
                if length > MAX_STRING {
                    return stop(
                        "fla_record_limit",
                        "Historical action extra data exceeds 1 MiB.",
                    );
                }
                put("extra_data", TimelineActionValue::Bytes(self.span(length)?));
            }
            Ok(())
        })();
        self.command_finish(commands, index, result)
    }

    fn metadata(
        &mut self,
        command: &mut TimelineActionCommand,
        quote_character: bool,
    ) -> Result<()> {
        for name in ["source_prefix", "source_text", "source_suffix"] {
            let value = TimelineActionValue::String(self.string()?);
            push(command, name, value);
        }
        push(
            command,
            "value40",
            TimelineActionValue::Dword(self.dword()?),
        );
        if quote_character {
            push(
                command,
                "value_quote_character",
                TimelineActionValue::Word(self.word()?),
            );
        }
        let value = TimelineActionValue::Dword(self.dword()?);
        push(command, "suppress_block_delimiter", value);
        Ok(())
    }

    fn parameter(&mut self, command: &mut TimelineActionCommand) -> Result<()> {
        let text = self.string()?;
        let flags = self.dword()?;
        push(
            command,
            "parameter",
            TimelineActionValue::Parameter { text, flags },
        );
        Ok(())
    }

    fn compiled_revision(&mut self, command: &mut TimelineActionCommand, max: u32) -> Result<u32> {
        let revision = self.dword()?;
        command.revision = Some(revision);
        if revision > max {
            return stop(
                "fla_action_command_revision_unsupported",
                format!(
                    "Compiled action command {} revision {revision} is not implemented.",
                    command.kind
                ),
            );
        }
        Ok(revision)
    }

    fn compiled_command(
        &mut self,
        commands: &mut Vec<TimelineActionCommand>,
        list_revision: u32,
    ) -> Result<()> {
        let index = self.command_start(commands);
        let result = (|| {
            let command = &mut commands[index];
            let kind = self.dword()?;
            command.kind = kind;
            if simple_compiled(kind) {
                command.body_present = list_revision != 0;
                if command.body_present && self.compiled_revision(command, 2)? >= 2 {
                    self.metadata(command, false)?;
                }
                return Ok(());
            }
            match kind {
                0 => {
                    let revision = self.compiled_revision(command, 2)?;
                    push(
                        command,
                        "navigation_mode",
                        TimelineActionValue::Dword(self.dword()?),
                    );
                    push(
                        command,
                        "scene",
                        TimelineActionValue::String(self.string()?),
                    );
                    push(
                        command,
                        "frame",
                        TimelineActionValue::String(self.string()?),
                    );
                    push(command, "play", TimelineActionValue::Dword(self.dword()?));
                    if revision >= 2 {
                        self.metadata(command, false)?;
                    }
                    return Ok(());
                }
                5 => {
                    let revision = self.compiled_revision(command, 2)?;
                    push(command, "url", TimelineActionValue::String(self.string()?));
                    push(
                        command,
                        "window",
                        TimelineActionValue::String(self.string()?),
                    );
                    for name in ["url_flags", "window_flags", "method"] {
                        push(command, name, TimelineActionValue::Dword(self.dword()?));
                    }
                    if revision >= 2 {
                        self.metadata(command, false)?;
                    }
                    return Ok(());
                }
                11 | 13 => {
                    let revision = self.compiled_revision(command, 2)?;
                    push(
                        command,
                        "event_flags",
                        TimelineActionValue::Dword(self.dword()?),
                    );
                    if kind == 11 {
                        push(
                            command,
                            "key_code",
                            TimelineActionValue::Dword(self.dword()?),
                        );
                        if revision >= 1 {
                            push(
                                command,
                                "event_string",
                                TimelineActionValue::String(self.string()?),
                            );
                        }
                    }
                    if revision >= 2 {
                        self.metadata(command, false)?;
                    }
                    return Ok(());
                }
                _ => {}
            }
            let Some((max, _, _)) = (kind != 35).then(|| compiled_layout(kind, 0)).flatten() else {
                return stop(
                    "fla_action_command_unsupported",
                    format!("Compiled action command type {kind} has no known layout."),
                );
            };
            command.body_present = list_revision != 0 || !matches!(kind, 46 | 82);
            if !command.body_present {
                return Ok(());
            }
            let revision = self.compiled_revision(command, max)?;
            let (_, meta, ops) = compiled_layout(kind, revision).expect("known layout");
            match kind {
                46 | 82 => {
                    if revision >= 2 {
                        self.metadata(command, false)?;
                        self.parameter(command)?;
                    }
                    return Ok(());
                }
                76 | 78 | 83 => {
                    if revision >= 2 {
                        let names: [&str; 3] = match kind {
                            76 => ["package_name", "", ""],
                            _ => ["namespace_name", "namespace_value", "namespace_qualifier"],
                        };
                        push(
                            command,
                            names[0],
                            TimelineActionValue::String(self.string()?),
                        );
                        push(
                            command,
                            "value40",
                            TimelineActionValue::Dword(self.dword()?),
                        );
                        let value = TimelineActionValue::Dword(self.dword()?);
                        push(command, "suppress_block_delimiter", value);
                        if revision >= 3 {
                            let a = if kind == 76 { "string2" } else { names[1] };
                            let b = if kind == 76 { "string3" } else { names[2] };
                            push(command, a, TimelineActionValue::String(self.string()?));
                            push(command, b, TimelineActionValue::String(self.string()?));
                        }
                    }
                    return Ok(());
                }
                _ => {}
            }
            for op in ops {
                match op {
                    Op::Str(name) => {
                        push(command, name, TimelineActionValue::String(self.string()?))
                    }
                    Op::Word(name) => {
                        push(command, name, TimelineActionValue::Dword(self.dword()?))
                    }
                    Op::Param => self.parameter(command)?,
                    Op::WordFrom(r, name) if revision >= r => {
                        push(command, name, TimelineActionValue::Dword(self.dword()?))
                    }
                    Op::ByteFrom(r, name) if revision >= r => {
                        push(command, name, TimelineActionValue::Byte(self.byte()?))
                    }
                    _ => {}
                }
            }
            if revision >= meta {
                self.metadata(command, kind == 24)?;
                if kind == 28 {
                    push(
                        command,
                        "comment_style",
                        TimelineActionValue::Dword(self.dword()?),
                    );
                }
            }
            Ok(())
        })();
        self.command_finish(commands, index, result)
    }
}

fn push(command: &mut TimelineActionCommand, name: &str, value: TimelineActionValue) {
    command.fields.push(TimelineActionField {
        name: name.into(),
        value,
    });
}
