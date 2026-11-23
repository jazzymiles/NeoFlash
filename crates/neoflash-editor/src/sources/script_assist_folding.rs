use super::*;

/// Deterministic brace/block fold range (1-based inclusive line numbers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldRegion {
    pub start_line: usize,
    pub end_line: usize,
    pub label: String,
}

/// Bounded language-aware Script Assist templates for structured statement insertion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptAssistTemplate {
    StopTimeline,
    GotoAndPlayFrame {
        frame: u32,
    },
    GotoAndStopFrame {
        frame: u32,
    },
    OnReleaseHandler {
        body: String,
    },
    EnterFrameLoop {
        body: String,
    },
    TraceExpression {
        expression: String,
    },
    VariableDeclaration {
        name: String,
        var_type: String,
        initial_value: String,
    },
    DegreesRadiansHelpers,
    DistanceAndAngleHelpers,
    RandomClampLerpHelpers,
    EaseAndSpringFollow,
    ShuffleArrayHelper,
    RgbHexHelpers,
    FormatTimeHelper,
    KeyStateTracker,
    SharedObjectSaveLoad,
}

impl ScriptAssistTemplate {
    pub(crate) fn default_menu_items() -> Vec<(&'static str, Self)> {
        vec![
            ("Stop timeline", Self::StopTimeline),
            ("Goto and play frame…", Self::GotoAndPlayFrame { frame: 1 }),
            ("Goto and stop frame…", Self::GotoAndStopFrame { frame: 1 }),
            (
                "On click / release handler",
                Self::OnReleaseHandler {
                    body: "stop();".into(),
                },
            ),
            (
                "Enter frame loop",
                Self::EnterFrameLoop {
                    body: "// frame update".into(),
                },
            ),
            (
                "Trace / log expression",
                Self::TraceExpression {
                    expression: "\"trace\"".into(),
                },
            ),
            (
                "Declare variable",
                Self::VariableDeclaration {
                    name: "value".into(),
                    var_type: "Number".into(),
                    initial_value: "0".into(),
                },
            ),
            (
                "Degrees ↔ radians (degToRad / radToDeg)",
                Self::DegreesRadiansHelpers,
            ),
            (
                "Distance & angle (atan2) helpers",
                Self::DistanceAndAngleHelpers,
            ),
            (
                "Random range, clamp, lerp & map",
                Self::RandomClampLerpHelpers,
            ),
            ("Ease & spring motion step", Self::EaseAndSpringFollow),
            ("Shuffle array (Fisher–Yates)", Self::ShuffleArrayHelper),
            ("RGB ↔ Hex color conversion", Self::RgbHexHelpers),
            ("Format MM:SS timer (padZero)", Self::FormatTimeHelper),
            ("Keyboard state tracker (keysDown)", Self::KeyStateTracker),
            ("Save / load SharedObject", Self::SharedObjectSaveLoad),
        ]
    }
}

/// Format a `ScriptAssistTemplate` for the given `ScriptLanguage`.
pub fn format_script_assist_snippet(
    template: &ScriptAssistTemplate,
    language: ScriptLanguage,
) -> String {
    match template {
        ScriptAssistTemplate::StopTimeline => match language {
            ScriptLanguage::ActionScript1
            | ScriptLanguage::ActionScript2
            | ScriptLanguage::ActionScript3 => "stop();\n".to_owned(),
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => "this.stop();\n".to_owned(),
        },
        ScriptAssistTemplate::GotoAndPlayFrame { frame } => {
            let f = (*frame).max(1);
            match language {
                ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3 => format!("gotoAndPlay({f});\n"),
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                    format!("this.gotoAndPlay({f});\n")
                }
            }
        }
        ScriptAssistTemplate::GotoAndStopFrame { frame } => {
            let f = (*frame).max(1);
            match language {
                ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3 => format!("gotoAndStop({f});\n"),
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                    format!("this.gotoAndStop({f});\n")
                }
            }
        }
        ScriptAssistTemplate::OnReleaseHandler { body } => {
            let body_line = body.trim();
            match language {
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                    format!("on (release) {{\n    {body_line}\n}}\n")
                }
                ScriptLanguage::ActionScript3 => format!(
                    "this.addEventListener(MouseEvent.CLICK, function(event:MouseEvent):void {{\n    {body_line}\n}});\n"
                ),
                ScriptLanguage::JavaScript => format!(
                    "this.addEventListener(\"click\", (event) => {{\n    {body_line}\n}});\n"
                ),
                ScriptLanguage::TypeScript => format!(
                    "this.addEventListener(\"click\", (event: MouseEvent): void => {{\n    {body_line}\n}});\n"
                ),
            }
        }
        ScriptAssistTemplate::EnterFrameLoop { body } => {
            let body_line = body.trim();
            match language {
                ScriptLanguage::ActionScript1 => {
                    format!("this.onEnterFrame = function() {{\n    {body_line}\n}};\n")
                }
                ScriptLanguage::ActionScript2 => {
                    format!("this.onEnterFrame = function():Void {{\n    {body_line}\n}};\n")
                }
                ScriptLanguage::ActionScript3 => format!(
                    "this.addEventListener(Event.ENTER_FRAME, function(event:Event):void {{\n    {body_line}\n}});\n"
                ),
                ScriptLanguage::JavaScript => format!(
                    "this.addEventListener(\"enterFrame\", () => {{\n    {body_line}\n}});\n"
                ),
                ScriptLanguage::TypeScript => format!(
                    "this.addEventListener(\"enterFrame\", (): void => {{\n    {body_line}\n}});\n"
                ),
            }
        }
        ScriptAssistTemplate::TraceExpression { expression } => {
            let expr = expression.trim();
            match language {
                ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3 => format!("trace({expr});\n"),
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                    format!("console.log({expr});\n")
                }
            }
        }
        ScriptAssistTemplate::VariableDeclaration {
            name,
            var_type,
            initial_value,
        } => {
            let name = name.trim();
            let var_type = var_type.trim();
            let init = initial_value.trim();
            match language {
                ScriptLanguage::ActionScript1 => format!("var {name} = {init};\n"),
                ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => {
                    if var_type.is_empty() {
                        format!("var {name} = {init};\n")
                    } else {
                        format!("var {name}:{var_type} = {init};\n")
                    }
                }
                ScriptLanguage::JavaScript => format!("let {name} = {init};\n"),
                ScriptLanguage::TypeScript => {
                    if var_type.is_empty() {
                        format!("let {name} = {init};\n")
                    } else {
                        format!("let {name}: {var_type} = {init};\n")
                    }
                }
            }
        }
        ScriptAssistTemplate::DegreesRadiansHelpers => match language {
            ScriptLanguage::ActionScript3 => "var DEG_TO_RAD:Number = Math.PI / 180;\nvar RAD_TO_DEG:Number = 180 / Math.PI;\nfunction degToRad(deg:Number):Number { return deg * DEG_TO_RAD; }\nfunction radToDeg(rad:Number):Number { return rad * RAD_TO_DEG; }\n".to_owned(),
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => "var DEG_TO_RAD = Math.PI / 180;\nvar RAD_TO_DEG = 180 / Math.PI;\nfunction degToRad(deg) { return deg * DEG_TO_RAD; }\nfunction radToDeg(rad) { return rad * RAD_TO_DEG; }\n".to_owned(),
            ScriptLanguage::JavaScript => "const DEG_TO_RAD = Math.PI / 180;\nconst RAD_TO_DEG = 180 / Math.PI;\nfunction degToRad(deg) { return deg * DEG_TO_RAD; }\nfunction radToDeg(rad) { return rad * RAD_TO_DEG; }\n".to_owned(),
            ScriptLanguage::TypeScript => "const DEG_TO_RAD: number = Math.PI / 180;\nconst RAD_TO_DEG: number = 180 / Math.PI;\nfunction degToRad(deg: number): number { return deg * DEG_TO_RAD; }\nfunction radToDeg(rad: number): number { return rad * RAD_TO_DEG; }\n".to_owned(),
        },
        ScriptAssistTemplate::DistanceAndAngleHelpers => match language {
            ScriptLanguage::ActionScript3 => "function getDistance(x1:Number, y1:Number, x2:Number, y2:Number):Number {\n    var dx:Number = x2 - x1;\n    var dy:Number = y2 - y1;\n    return Math.sqrt(dx * dx + dy * dy);\n}\nfunction getAngleDegrees(x1:Number, y1:Number, x2:Number, y2:Number):Number {\n    return Math.atan2(y2 - y1, x2 - x1) * (180 / Math.PI);\n}\n".to_owned(),
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => "function getDistance(x1, y1, x2, y2) {\n    var dx = x2 - x1;\n    var dy = y2 - y1;\n    return Math.sqrt(dx * dx + dy * dy);\n}\nfunction getAngleDegrees(x1, y1, x2, y2) {\n    return Math.atan2(y2 - y1, x2 - x1) * (180 / Math.PI);\n}\n".to_owned(),
            ScriptLanguage::JavaScript => "function getDistance(x1, y1, x2, y2) {\n    const dx = x2 - x1;\n    const dy = y2 - y1;\n    return Math.sqrt(dx * dx + dy * dy);\n}\nfunction getAngleDegrees(x1, y1, x2, y2) {\n    return Math.atan2(y2 - y1, x2 - x1) * (180 / Math.PI);\n}\n".to_owned(),
            ScriptLanguage::TypeScript => "function getDistance(x1: number, y1: number, x2: number, y2: number): number {\n    const dx: number = x2 - x1;\n    const dy: number = y2 - y1;\n    return Math.sqrt(dx * dx + dy * dy);\n}\nfunction getAngleDegrees(x1: number, y1: number, x2: number, y2: number): number {\n    return Math.atan2(y2 - y1, x2 - x1) * (180 / Math.PI);\n}\n".to_owned(),
        },
        ScriptAssistTemplate::RandomClampLerpHelpers => match language {
            ScriptLanguage::ActionScript3 => "function randomRange(min:Number, max:Number):Number { return min + Math.random() * (max - min); }\nfunction randomInt(min:int, max:int):int { return Math.floor(Math.random() * (max - min + 1)) + min; }\nfunction clamp(val:Number, min:Number, max:Number):Number { return val < min ? min : (val > max ? max : val); }\nfunction lerp(start:Number, end:Number, t:Number):Number { return start + (end - start) * t; }\nfunction mapRange(val:Number, inMin:Number, inMax:Number, outMin:Number, outMax:Number):Number { return outMin + (outMax - outMin) * ((val - inMin) / (inMax - inMin)); }\n".to_owned(),
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => "function randomRange(min, max) { return min + Math.random() * (max - min); }\nfunction randomInt(min, max) { return Math.floor(Math.random() * (max - min + 1)) + min; }\nfunction clamp(val, min, max) { return val < min ? min : (val > max ? max : val); }\nfunction lerp(start, end, t) { return start + (end - start) * t; }\nfunction mapRange(val, inMin, inMax, outMin, outMax) { return outMin + (outMax - outMin) * ((val - inMin) / (inMax - inMin)); }\n".to_owned(),
            ScriptLanguage::JavaScript => "function randomRange(min, max) { return min + Math.random() * (max - min); }\nfunction randomInt(min, max) { return Math.floor(Math.random() * (max - min + 1)) + min; }\nfunction clamp(val, min, max) { return val < min ? min : (val > max ? max : val); }\nfunction lerp(start, end, t) { return start + (end - start) * t; }\nfunction mapRange(val, inMin, inMax, outMin, outMax) { return outMin + (outMax - outMin) * ((val - inMin) / (inMax - inMin)); }\n".to_owned(),
            ScriptLanguage::TypeScript => "function randomRange(min: number, max: number): number { return min + Math.random() * (max - min); }\nfunction randomInt(min: number, max: number): number { return Math.floor(Math.random() * (max - min + 1)) + min; }\nfunction clamp(val: number, min: number, max: number): number { return val < min ? min : (val > max ? max : val); }\nfunction lerp(start: number, end: number, t: number): number { return start + (end - start) * t; }\nfunction mapRange(val: number, inMin: number, inMax: number, outMin: number, outMax: number): number { return outMin + (outMax - outMin) * ((val - inMin) / (inMax - inMin)); }\n".to_owned(),
        },
        ScriptAssistTemplate::EaseAndSpringFollow => match language {
            ScriptLanguage::ActionScript3 => "var vx:Number = 0;\nvar vy:Number = 0;\nvar spring:Number = 0.12;\nvar friction:Number = 0.82;\nvx = (vx + (stage.mouseX - this.x) * spring) * friction;\nvy = (vy + (stage.mouseY - this.y) * spring) * friction;\nthis.x += vx;\nthis.y += vy;\n".to_owned(),
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => "var vx = 0;\nvar vy = 0;\nvar spring = 0.12;\nvar friction = 0.82;\nvx = (vx + (_root._xmouse - this._x) * spring) * friction;\nvy = (vy + (_root._ymouse - this._y) * spring) * friction;\nthis._x += vx;\nthis._y += vy;\n".to_owned(),
            ScriptLanguage::JavaScript => "let vx = 0;\nlet vy = 0;\nconst spring = 0.12;\nconst friction = 0.82;\nvx = (vx + (this.mouseX - this.x) * spring) * friction;\nvy = (vy + (this.mouseY - this.y) * spring) * friction;\nthis.x += vx;\nthis.y += vy;\n".to_owned(),
            ScriptLanguage::TypeScript => "let vx: number = 0;\nlet vy: number = 0;\nconst spring: number = 0.12;\nconst friction: number = 0.82;\nvx = (vx + (this.mouseX - this.x) * spring) * friction;\nvy = (vy + (this.mouseY - this.y) * spring) * friction;\nthis.x += vx;\nthis.y += vy;\n".to_owned(),
        },
        ScriptAssistTemplate::ShuffleArrayHelper => match language {
            ScriptLanguage::ActionScript3 => "function shuffleArray(arr:Array):Array {\n    var i:int = arr.length;\n    while (--i > 0) {\n        var j:int = Math.floor(Math.random() * (i + 1));\n        var tmp:* = arr[i];\n        arr[i] = arr[j];\n        arr[j] = tmp;\n    }\n    return arr;\n}\n".to_owned(),
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => "function shuffleArray(arr) {\n    var i = arr.length;\n    while (--i > 0) {\n        var j = Math.floor(Math.random() * (i + 1));\n        var tmp = arr[i];\n        arr[i] = arr[j];\n        arr[j] = tmp;\n    }\n    return arr;\n}\n".to_owned(),
            ScriptLanguage::JavaScript => "function shuffleArray(arr) {\n    let i = arr.length;\n    while (--i > 0) {\n        const j = Math.floor(Math.random() * (i + 1));\n        const tmp = arr[i];\n        arr[i] = arr[j];\n        arr[j] = tmp;\n    }\n    return arr;\n}\n".to_owned(),
            ScriptLanguage::TypeScript => "function shuffleArray(arr: any[]): any[] {\n    let i: number = arr.length;\n    while (--i > 0) {\n        const j: number = Math.floor(Math.random() * (i + 1));\n        const tmp: any = arr[i];\n        arr[i] = arr[j];\n        arr[j] = tmp;\n    }\n    return arr;\n}\n".to_owned(),
        },
        ScriptAssistTemplate::RgbHexHelpers => match language {
            ScriptLanguage::ActionScript3 => "function rgbToHex(r:uint, g:uint, b:uint):uint { return ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF); }\nfunction hexToRgb(hex:uint):Object { return { r: (hex >> 16) & 0xFF, g: (hex >> 8) & 0xFF, b: hex & 0xFF }; }\n".to_owned(),
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => "function rgbToHex(r, g, b) { return ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF); }\nfunction hexToRgb(hex) { return { r: (hex >> 16) & 0xFF, g: (hex >> 8) & 0xFF, b: hex & 0xFF }; }\n".to_owned(),
            ScriptLanguage::JavaScript => "function rgbToHex(r, g, b) { return ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF); }\nfunction hexToRgb(hex) { return { r: (hex >> 16) & 0xFF, g: (hex >> 8) & 0xFF, b: hex & 0xFF }; }\n".to_owned(),
            ScriptLanguage::TypeScript => "function rgbToHex(r: number, g: number, b: number): number { return ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF); }\nfunction hexToRgb(hex: number): { r: number; g: number; b: number } { return { r: (hex >> 16) & 0xFF, g: (hex >> 8) & 0xFF, b: hex & 0xFF }; }\n".to_owned(),
        },
        ScriptAssistTemplate::FormatTimeHelper => match language {
            ScriptLanguage::ActionScript3 => "function padZero(n:int):String { return (n < 10 ? \"0\" : \"\") + n; }\nfunction formatTimeMMSS(totalSeconds:Number):String {\n    var mins:int = int(Math.floor(totalSeconds / 60));\n    var secs:int = int(Math.floor(totalSeconds % 60));\n    return padZero(mins) + \":\" + padZero(secs);\n}\n".to_owned(),
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => "function padZero(n) { return (n < 10 ? \"0\" : \"\") + n; }\nfunction formatTimeMMSS(totalSeconds) {\n    var mins = Math.floor(totalSeconds / 60);\n    var secs = Math.floor(totalSeconds % 60);\n    return padZero(mins) + \":\" + padZero(secs);\n}\n".to_owned(),
            ScriptLanguage::JavaScript => "function padZero(n) { return (n < 10 ? \"0\" : \"\") + n; }\nfunction formatTimeMMSS(totalSeconds) {\n    const mins = Math.floor(totalSeconds / 60);\n    const secs = Math.floor(totalSeconds % 60);\n    return padZero(mins) + \":\" + padZero(secs);\n}\n".to_owned(),
            ScriptLanguage::TypeScript => "function padZero(n: number): string { return (n < 10 ? \"0\" : \"\") + n; }\nfunction formatTimeMMSS(totalSeconds: number): string {\n    const mins: number = Math.floor(totalSeconds / 60);\n    const secs: number = Math.floor(totalSeconds % 60);\n    return padZero(mins) + \":\" + padZero(secs);\n}\n".to_owned(),
        },
        ScriptAssistTemplate::KeyStateTracker => match language {
            ScriptLanguage::ActionScript3 => "var keysDown:Object = {};\nstage.addEventListener(KeyboardEvent.KEY_DOWN, function(e:KeyboardEvent):void { keysDown[e.keyCode] = true; });\nstage.addEventListener(KeyboardEvent.KEY_UP, function(e:KeyboardEvent):void { keysDown[e.keyCode] = false; });\n".to_owned(),
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => "var keysDown = {};\nvar keyStateListener = {};\nkeyStateListener.onKeyDown = function() { keysDown[Key.getCode()] = true; };\nkeyStateListener.onKeyUp = function() { keysDown[Key.getCode()] = false; };\nKey.addListener(keyStateListener);\n".to_owned(),
            ScriptLanguage::JavaScript => "const keysDown = {};\nwindow.addEventListener(\"keydown\", (e) => { keysDown[e.keyCode] = true; });\nwindow.addEventListener(\"keyup\", (e) => { keysDown[e.keyCode] = false; });\n".to_owned(),
            ScriptLanguage::TypeScript => "const keysDown: any = {};\nwindow.addEventListener(\"keydown\", (e: KeyboardEvent): void => { keysDown[e.keyCode] = true; });\nwindow.addEventListener(\"keyup\", (e: KeyboardEvent): void => { keysDown[e.keyCode] = false; });\n".to_owned(),
        },
        ScriptAssistTemplate::SharedObjectSaveLoad => match language {
            ScriptLanguage::ActionScript1 => "var saveStore = SharedObject.getLocal(\"neoflash_save\");\nsaveStore.data.highScore = 1000;\nsaveStore.flush();\n".to_owned(),
            ScriptLanguage::ActionScript3 | ScriptLanguage::ActionScript2 => "var saveStore:SharedObject = SharedObject.getLocal(\"neoflash_save\");\nsaveStore.data.highScore = 1000;\nsaveStore.flush();\n".to_owned(),
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => "window.localStorage.setItem(\"neoflash_highScore\", \"1000\");\n".to_owned(),
        },
    }
}

pub(crate) fn append_script_assist_to_source(source: &mut String, snippet: &str) {
    if !source.is_empty() && !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str(snippet);
}

/// Compute deterministic brace/block folding regions (`{ ... }`) for `source`, ignoring braces
/// inside comments and string literals.
pub fn compute_fold_regions(source: &str) -> Vec<FoldRegion> {
    let lines: Vec<&str> = source.split('\n').collect();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut regions: Vec<FoldRegion> = Vec::new();
    let mut in_block_comment = false;

    for (idx, raw_line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let chars: Vec<char> = raw_line.chars().collect();
        let mut i = 0;
        let mut in_string: Option<char> = None;

        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();

            if in_block_comment {
                if c == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if let Some(quote) = in_string {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == quote {
                    in_string = None;
                }
                i += 1;
                continue;
            }

            if c == '/' && next == Some('/') {
                break;
            }
            if c == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }
            if c == '"' || c == '\'' {
                in_string = Some(c);
                i += 1;
                continue;
            }

            if c == '{' {
                let header = raw_line.trim();
                let prefix = header.strip_suffix('{').unwrap_or(header).trim();
                let label = if prefix.is_empty() {
                    "{ … }".to_owned()
                } else {
                    format!("{prefix} {{ … }}")
                };
                stack.push((line_no, label));
            } else if c == '}'
                && let Some((start_line, label)) = stack.pop()
                && line_no > start_line
            {
                regions.push(FoldRegion {
                    start_line,
                    end_line: line_no,
                    label,
                });
            }
            i += 1;
        }
    }

    regions.sort_by_key(|r| (r.start_line, r.end_line));
    regions
}

impl Editor {
    pub(super) fn active_fold_tab_key(&self) -> String {
        use crate::panels::actions_sidebar::ScriptLocation;
        match self.effective_actions_location() {
            Some(ScriptLocation::Source(id)) => {
                if let Some(unit) = self.project.sources.iter().find(|s| s.id == id) {
                    return unit.path.clone();
                }
                format!("source:{id}")
            }
            Some(ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            }) => match layer {
                Some(l) => format!("{symbol}:{frame}:frame:{l}"),
                None => format!("{symbol}:{frame}:frame"),
            },
            Some(ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            }) => format!("{symbol}:{frame}:object:{element}:{event:?}"),
            None => format!("{}:{}:frame", self.symbol_id(), self.frame),
        }
    }

    pub fn folded_lines(&self) -> std::collections::BTreeSet<usize> {
        let key = self.active_fold_tab_key();
        self.with_authoring_state(|state| {
            state
                .folded_lines_by_tab
                .get(&key)
                .cloned()
                .unwrap_or_default()
        })
    }

    pub(crate) fn toggle_fold_line_for(&mut self, tab_key: &str, line: usize) -> bool {
        let line = line.max(1);
        self.with_authoring_state_mut(|state| {
            let set = state
                .folded_lines_by_tab
                .entry(tab_key.to_owned())
                .or_default();
            if set.contains(&line) {
                set.remove(&line);
                false
            } else {
                set.insert(line);
                true
            }
        })
    }

    /// Toggle code folding at 1-based `line` for the active script tab. Returns `true` if now folded.
    pub fn toggle_fold_line(&mut self, line: usize) -> bool {
        let key = self.active_fold_tab_key();
        self.toggle_fold_line_for(&key, line)
    }

    pub fn expand_all_folds(&mut self) {
        let key = self.active_fold_tab_key();
        self.with_authoring_state_mut(|state| {
            state.folded_lines_by_tab.remove(&key);
        });
    }

    pub(crate) fn collapse_between_braces_in_current_editor(
        &mut self,
        ctx: &egui::Context,
        fallback_source: &str,
    ) -> bool {
        let source = if let Some(crate::panels::actions_sidebar::ScriptLocation::Source(id)) =
            self.effective_actions_location()
        {
            self.project
                .sources
                .iter()
                .find(|s| s.id == id)
                .map_or(fallback_source, |s| s.source.as_str())
                .to_owned()
        } else {
            fallback_source.to_owned()
        };
        let regions = compute_fold_regions(&source);
        if regions.is_empty() {
            return false;
        }
        let editor_id = self.actions_editor_id();
        let caret_line = egui::text_edit::TextEditState::load(ctx, editor_id)
            .and_then(|s| s.cursor.char_range().map(|r| r.primary.index))
            .map_or(1, |idx| nf::code::line_col(&source, idx).0);
        let target_line = regions
            .iter()
            .filter(|r| r.start_line <= caret_line && caret_line <= r.end_line)
            .max_by_key(|r| r.start_line)
            .or_else(|| regions.first())
            .map(|r| r.start_line);
        if let Some(line) = target_line {
            self.toggle_fold_line(line)
        } else {
            false
        }
    }

    /// Return the visible 1-based `(line_number, line_text)` pairs after applying active fold regions.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn visible_source_lines(&self, source: &str) -> Vec<(usize, String)> {
        let regions = compute_fold_regions(source);
        let folded = self.folded_lines();
        let active_regions: Vec<&FoldRegion> = regions
            .iter()
            .filter(|r| folded.contains(&r.start_line))
            .collect();

        let mut visible = Vec::new();
        for (idx, raw_line) in source.lines().enumerate() {
            let line_no = idx + 1;
            if active_regions
                .iter()
                .any(|r| r.start_line < line_no && line_no <= r.end_line)
            {
                continue;
            }
            if let Some(region) = active_regions.iter().find(|r| r.start_line == line_no) {
                visible.push((line_no, region.label.clone()));
            } else {
                visible.push((line_no, raw_line.to_owned()));
            }
        }
        visible
    }

    /// Insert a bounded Script Assist statement into the active frame, object, or project source script.
    pub fn insert_script_assist_snippet(&mut self, template: ScriptAssistTemplate) -> bool {
        use crate::panels::actions_sidebar::ScriptLocation;
        match self.effective_actions_location() {
            Some(ScriptLocation::Source(selected_id)) => {
                let Some(unit) = self.project.sources.iter().find(|s| s.id == selected_id) else {
                    return false;
                };
                let snippet = format_script_assist_snippet(&template, unit.language);
                self.transact_named("Script assist", |editor| {
                    if let Some(u) = editor
                        .project
                        .sources
                        .iter_mut()
                        .find(|s| s.id == selected_id)
                    {
                        append_script_assist_to_source(&mut u.source, &snippet);
                    }
                });
                true
            }
            Some(ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            }) => {
                let Some(sym) = self.project.symbol(&symbol) else {
                    return false;
                };
                let layer_index = layer
                    .as_deref()
                    .and_then(|id| sym.timeline.layers.iter().position(|l| l.id == id))
                    .or_else(|| sym.timeline.action_layer_for_frame(frame))
                    .unwrap_or(0);
                let Some(layer_id) = sym.timeline.layers.get(layer_index).map(|l| l.id.clone())
                else {
                    return false;
                };
                let default_lang = self.default_script_language_for_project();
                let language = sym
                    .timeline
                    .frame_script_for_layer(layer_index, frame)
                    .map_or(default_lang, |s| s.script.language);
                let snippet = format_script_assist_snippet(&template, language);
                self.transact_named("Script assist", |editor| {
                    let prev_editing = editor.editing.clone();
                    if editor.symbol_id() != symbol {
                        editor.editing = vec![symbol.clone()];
                    }
                    editor.ensure_frame_script_key_at(&layer_id, frame);
                    editor.editing = prev_editing;
                    let Some(sym_mut) = editor.project.symbol_mut(&symbol) else {
                        return;
                    };
                    let existing = sym_mut
                        .timeline
                        .frame_script_index_for_layer(layer_index, frame);
                    let scripts = &mut sym_mut.timeline.scripts;
                    if let Some(index) = existing {
                        let existing = &mut scripts[index];
                        existing.layer = Some(layer_id.clone());
                        existing.script.language = language;
                        append_script_assist_to_source(&mut existing.script.source, &snippet);
                    } else {
                        scripts.push(neoflash_core::FrameScript {
                            layer: Some(layer_id.clone()),
                            frame,
                            script: Script {
                                language,
                                source: snippet,
                            },
                        });
                        scripts.sort_by_key(|script| script.frame);
                    }
                });
                true
            }
            Some(ScriptLocation::Object {
                symbol,
                element,
                event,
                ..
            }) => {
                let editing_pinned = self.actions_ui.active_pinned.is_some();
                if !editing_pinned && !self.selected_editable() {
                    return false;
                }
                let default_lang = self.default_script_language_for_project();
                let Some(language) = self
                    .project
                    .symbol(&symbol)
                    .and_then(|sym| {
                        sym.timeline
                            .layers
                            .iter()
                            .flat_map(|l| &l.keyframes)
                            .flat_map(|k| &k.elements)
                            .find(|e| e.id == element)
                    })
                    .map(|e| {
                        e.scripts
                            .iter()
                            .find(|s| s.event == event && !s.script.source.trim().is_empty())
                            .map_or(default_lang, |s| s.script.language)
                    })
                else {
                    return false;
                };
                let snippet = format_script_assist_snippet(&template, language);
                self.transact_named("Script assist", |editor| {
                    if editing_pinned {
                        if let Some(sym_mut) = editor.project.symbol_mut(&symbol) {
                            for l in &mut sym_mut.timeline.layers {
                                for k in &mut l.keyframes {
                                    if let Some(el) =
                                        k.elements.iter_mut().find(|e| e.id == element)
                                    {
                                        if let Some(existing) =
                                            el.scripts.iter_mut().find(|s| s.event == event)
                                        {
                                            existing.script.language = language;
                                            append_script_assist_to_source(
                                                &mut existing.script.source,
                                                &snippet,
                                            );
                                        } else {
                                            el.scripts.push(neoflash_core::ObjectScript {
                                                event,
                                                script: Script {
                                                    language,
                                                    source: snippet.clone(),
                                                },
                                            });
                                        }
                                        return;
                                    }
                                }
                            }
                        }
                    } else {
                        editor.ensure_selected_key();
                        if let Some(el) = editor.selected_element_mut() {
                            if let Some(existing) = el.scripts.iter_mut().find(|s| s.event == event)
                            {
                                existing.script.language = language;
                                append_script_assist_to_source(
                                    &mut existing.script.source,
                                    &snippet,
                                );
                            } else {
                                el.scripts.push(neoflash_core::ObjectScript {
                                    event,
                                    script: Script {
                                        language,
                                        source: snippet,
                                    },
                                });
                            }
                        }
                    }
                });
                true
            }
            None => false,
        }
    }

    /// Project-level script language (`R-102`) for newly created frame, object, or event scripts.
    pub fn default_script_language_for_project(&self) -> ScriptLanguage {
        self.project.effective_script_language()
    }
}
