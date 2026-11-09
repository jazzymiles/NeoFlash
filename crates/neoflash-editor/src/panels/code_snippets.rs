//! Code snippets panel (`Panel / Code snippets`, `PP-03`; reference `scripting-panels.png`),
//! opened by Window ▸ Panels ▸ Code snippets.
//!
//! Displays the six NF2 snippet categories, live search, a language-aware preview adapted to the
//! selected stage instance (defaulting to `orbit_mc`), `Copy` to clipboard, and `Insert at frame N`
//! in a single `"Insert code snippet"` Undo transaction.
use super::planned::caption;
use crate::app::{Editor, ScriptTarget};
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::{ElementKind, FrameScript, Script, ScriptLanguage, SymbolKind};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

pub(crate) const SNIPPET_CATEGORIES: [&str; 11] = [
    "Actions",
    "Timeline navigation",
    "Animation",
    "Load and unload",
    "Audio and video",
    "Event handlers",
    "Math and geometry",
    "Physics and movement",
    "Collisions and stage",
    "Arrays, strings and color",
    "Preloaders, storage and debug",
];

pub(crate) struct CodeSnippetDef {
    pub category: usize,
    pub title: &'static str,
    pub description: &'static str,
    pub uses_target: bool,
    pub as3_imports: &'static [&'static str],
    pub as3: &'static str,
    pub as2: &'static str,
    pub js: &'static str,
    pub ts: &'static str,
}

/// Default selected snippet in the NF2 reference (`"Rotate continuously"` under `"Animation"`).
pub(crate) const DEFAULT_SNIPPET_INDEX: usize = 6;

pub(crate) const CODE_SNIPPETS: [CodeSnippetDef; 80] = [
    // 0: Actions (indices 0..3 preserved for NF2 reference & tests)
    CodeSnippetDef {
        category: 0,
        title: "Click to go to frame and stop",
        description: "Moves the playhead to frame 5 and stops when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onGoToAndStop);\nfunction onGoToAndStop(e:MouseEvent):void {\n  gotoAndStop(5);\n}",
        as2: "{target}.onRelease = function() {\n  gotoAndStop(5);\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.gotoAndStop(5);\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.gotoAndStop(5);\n});",
    },
    CodeSnippetDef {
        category: 0,
        title: "Click to go to web page",
        description: "Opens a URL in a new browser tab when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &[
            "flash.events.MouseEvent",
            "flash.net.URLRequest",
            "flash.net.navigateToURL",
        ],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onOpenWebPage);\nfunction onOpenWebPage(e:MouseEvent):void {\n  navigateToURL(new URLRequest(\"https://example.com\"), \"_blank\");\n}",
        as2: "{target}.onRelease = function() {\n  getURL(\"https://example.com\", \"_blank\");\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  window.open(\"https://example.com\", \"_blank\");\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  window.open(\"https://example.com\", \"_blank\");\n});",
    },
    CodeSnippetDef {
        category: 0,
        title: "Drag and drop",
        description: "Makes the selected movie clip draggable while the mouse button is pressed.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.MOUSE_DOWN, onStartDrag);\n{target}.addEventListener(MouseEvent.MOUSE_UP, onStopDrag);\nfunction onStartDrag(e:MouseEvent):void {\n  {target}.startDrag();\n}\nfunction onStopDrag(e:MouseEvent):void {\n  {target}.stopDrag();\n}",
        as2: "{target}.onPress = function() {\n  {target}.startDrag();\n};\n{target}.onRelease = function() {\n  {target}.stopDrag();\n};",
        js: "this.{target}.addEventListener(\"mouseDown\", () => {\n  this.{target}.startDrag();\n});\nthis.{target}.addEventListener(\"mouseUp\", () => {\n  this.{target}.stopDrag();\n});",
        ts: "this.{target}.addEventListener(\"mouseDown\", (): void => {\n  this.{target}.startDrag();\n});\nthis.{target}.addEventListener(\"mouseUp\", (): void => {\n  this.{target}.stopDrag();\n});",
    },
    // 1: Timeline navigation (indices 3..6 preserved)
    CodeSnippetDef {
        category: 1,
        title: "Stop at this frame",
        description: "Stops timeline playback when the playhead enters this frame.",
        uses_target: false,
        as3_imports: &[],
        as3: "stop();",
        as2: "stop();",
        js: "this.stop();",
        ts: "this.stop();",
    },
    CodeSnippetDef {
        category: 1,
        title: "Click to go to frame and play",
        description: "Jumps to frame 1 and resumes playback when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onGoToAndPlay);\nfunction onGoToAndPlay(e:MouseEvent):void {\n  gotoAndPlay(1);\n}",
        as2: "{target}.onRelease = function() {\n  gotoAndPlay(1);\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.gotoAndPlay(1);\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.gotoAndPlay(1);\n});",
    },
    CodeSnippetDef {
        category: 1,
        title: "Click to go to next frame and stop",
        description: "Advances the timeline by one frame when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onNextFrame);\nfunction onNextFrame(e:MouseEvent):void {\n  nextFrame();\n}",
        as2: "{target}.onRelease = function() {\n  nextFrame();\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.nextFrame();\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.nextFrame();\n});",
    },
    // 2: Animation (indices 6..8 preserved)
    CodeSnippetDef {
        category: 2,
        title: "Rotate continuously",
        description: "Rotates the selected movie clip every frame. Change 10 to adjust speed.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(\n  Event.ENTER_FRAME, spin);\nfunction spin(e:Event):void {\n  {target}.rotation += 10;\n}",
        as2: "{target}.onEnterFrame = function() {\n  {target}._rotation += 10;\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  this.{target}.rotation += 10;\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  this.{target}.rotation += 10;\n});",
    },
    CodeSnippetDef {
        category: 2,
        title: "Fade in movie clip",
        description: "Gradually increases alpha every frame until the movie clip is fully opaque.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.alpha = 0;\n{target}.addEventListener(Event.ENTER_FRAME, onFadeIn);\nfunction onFadeIn(e:Event):void {\n  if ({target}.alpha < 1) {\n    {target}.alpha += 0.05;\n  }\n}",
        as2: "{target}._alpha = 0;\n{target}.onEnterFrame = function() {\n  if ({target}._alpha < 100) {\n    {target}._alpha += 5;\n  }\n};",
        js: "this.{target}.alpha = 0;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  if (this.{target}.alpha < 1) {\n    this.{target}.alpha += 0.05;\n  }\n});",
        ts: "this.{target}.alpha = 0;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  if (this.{target}.alpha < 1) {\n    this.{target}.alpha += 0.05;\n  }\n});",
    },
    // 3: Load and unload (indices 8..10 preserved)
    CodeSnippetDef {
        category: 3,
        title: "Click to load image",
        description: "Loads an external image asset onto the display list when clicked.",
        uses_target: true,
        as3_imports: &[
            "flash.display.Loader",
            "flash.events.MouseEvent",
            "flash.net.URLRequest",
        ],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onLoadImage);\nfunction onLoadImage(e:MouseEvent):void {\n  var loader:Loader = new Loader();\n  loader.load(new URLRequest(\"assets/image.png\"));\n  addChild(loader);\n}",
        as2: "{target}.onRelease = function() {\n  {target}.loadMovie(\"assets/image.png\");\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.{target}.load(\"assets/image.png\");\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.{target}.load(\"assets/image.png\");\n});",
    },
    CodeSnippetDef {
        category: 3,
        title: "Click to unload content",
        description: "Unloads external content or removes the target clip when clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onRemoveClip);\nfunction onRemoveClip(e:MouseEvent):void {\n  if ({target}.parent != null) {\n    {target}.parent.removeChild({target});\n  }\n}",
        as2: "{target}.onRelease = function() {\n  {target}.unloadMovie();\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  if (this.{target}.parent) {\n    this.{target}.parent.removeChild(this.{target});\n  }\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  if (this.{target}.parent) {\n    this.{target}.parent.removeChild(this.{target});\n  }\n});",
    },
    // 4: Audio and video (indices 10..12 preserved)
    CodeSnippetDef {
        category: 4,
        title: "Click to play or stop sound",
        description: "Toggles playback of an audio file when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &[
            "flash.events.MouseEvent",
            "flash.media.Sound",
            "flash.media.SoundChannel",
            "flash.net.URLRequest",
        ],
        as3: "var soundClip:Sound = new Sound();\nsoundClip.load(new URLRequest(\"assets/audio.mp3\"));\nvar soundChannel:SoundChannel = null;\n{target}.addEventListener(MouseEvent.CLICK, onToggleSound);\nfunction onToggleSound(e:MouseEvent):void {\n  if (soundChannel == null) {\n    soundChannel = soundClip.play();\n  } else {\n    soundChannel.stop();\n    soundChannel = null;\n  }\n}",
        as2: "var soundClip:Sound = new Sound();\nsoundClip.loadSound(\"assets/audio.mp3\", false);\nvar soundPlaying:Boolean = false;\n{target}.onRelease = function() {\n  if (soundPlaying) {\n    soundClip.stop();\n    soundPlaying = false;\n  } else {\n    soundClip.start();\n    soundPlaying = true;\n  }\n};",
        js: "const audio = new Audio(\"assets/audio.mp3\");\nthis.{target}.addEventListener(\"click\", () => {\n  if (audio.paused) {\n    audio.play();\n  } else {\n    audio.pause();\n  }\n});",
        ts: "const audio: HTMLAudioElement = new Audio(\"assets/audio.mp3\");\nthis.{target}.addEventListener(\"click\", (): void => {\n  if (audio.paused) {\n    audio.play();\n  } else {\n    audio.pause();\n  }\n});",
    },
    CodeSnippetDef {
        category: 4,
        title: "Click to stop all sounds",
        description: "Stops all currently playing sounds when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent", "flash.media.SoundMixer"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onStopAllSounds);\nfunction onStopAllSounds(e:MouseEvent):void {\n  SoundMixer.stopAll();\n}",
        as2: "var allSounds:Sound = new Sound();\n{target}.onRelease = function() {\n  allSounds.stop();\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.stopAllSounds();\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.stopAllSounds();\n});",
    },
    // 5: Event handlers (indices 12..14 preserved)
    CodeSnippetDef {
        category: 5,
        title: "Mouse click event",
        description: "Runs custom handler code whenever the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onMouseClick);\nfunction onMouseClick(e:MouseEvent):void {\n  trace(\"Clicked\");\n}",
        as2: "{target}.onRelease = function() {\n  trace(\"Clicked\");\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  console.log(\"Clicked\");\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  console.log(\"Clicked\");\n});",
    },
    CodeSnippetDef {
        category: 5,
        title: "Enter frame event",
        description: "Runs custom handler code on every frame tick of the timeline.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(Event.ENTER_FRAME, onFrameTick);\nfunction onFrameTick(e:Event):void {\n  trace(\"Frame tick\");\n}",
        as2: "{target}.onEnterFrame = function() {\n  trace(\"Frame tick\");\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  console.log(\"Frame tick\");\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  console.log(\"Frame tick\");\n});",
    },
    // --- Extended Actions, Timeline, Animation, Load/Unload, Audio & Events (indices 14..) ---
    CodeSnippetDef {
        category: 0,
        title: "Drag constrained to rectangle",
        description: "Drags the selected movie clip within a bounded rectangular area (for sliders or joysticks).",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent", "flash.geom.Rectangle"],
        as3: "var dragBounds:Rectangle = new Rectangle(50, 50, 300, 200);\n{target}.addEventListener(MouseEvent.MOUSE_DOWN, onStartBoundedDrag);\nstage.addEventListener(MouseEvent.MOUSE_UP, onStopBoundedDrag);\nfunction onStartBoundedDrag(e:MouseEvent):void {\n  {target}.startDrag(false, dragBounds);\n}\nfunction onStopBoundedDrag(e:MouseEvent):void {\n  {target}.stopDrag();\n}",
        as2: "{target}.onPress = function() {\n  {target}.startDrag(false, 50, 50, 350, 250);\n};\n{target}.onRelease = {target}.onReleaseOutside = function() {\n  {target}.stopDrag();\n};",
        js: "this.{target}.addEventListener(\"mouseDown\", () => {\n  this.{target}.startDrag(false, 50, 50, 300, 200);\n});\nthis.addEventListener(\"mouseUp\", () => {\n  this.{target}.stopDrag();\n});",
        ts: "this.{target}.addEventListener(\"mouseDown\", (): void => {\n  this.{target}.startDrag(false, 50, 50, 300, 200);\n});\nthis.addEventListener(\"mouseUp\", (): void => {\n  this.{target}.stopDrag();\n});",
    },
    CodeSnippetDef {
        category: 0,
        title: "Show or hide movie clip",
        description: "Toggles the visibility of the selected movie clip when clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onToggleVisible);\nfunction onToggleVisible(e:MouseEvent):void {\n  {target}.visible = !{target}.visible;\n}",
        as2: "{target}.onRelease = function() {\n  {target}._visible = !{target}._visible;\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.{target}.visible = !this.{target}.visible;\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.{target}.visible = !this.{target}.visible;\n});",
    },
    CodeSnippetDef {
        category: 0,
        title: "Custom mouse cursor",
        description: "Hides the system pointer and locks the selected movie clip to the mouse coordinates.",
        uses_target: true,
        as3_imports: &["flash.events.Event", "flash.ui.Mouse"],
        as3: "Mouse.hide();\n{target}.mouseEnabled = false;\nstage.addEventListener(Event.ENTER_FRAME, onUpdateCursor);\nfunction onUpdateCursor(e:Event):void {\n  {target}.x = stage.mouseX;\n  {target}.y = stage.mouseY;\n}",
        as2: "Mouse.hide();\n{target}.onEnterFrame = function() {\n  {target}._x = _root._xmouse;\n  {target}._y = _root._ymouse;\n};",
        js: "this.addEventListener(\"enterFrame\", () => {\n  this.{target}.x = this.mouseX;\n  this.{target}.y = this.mouseY;\n});",
        ts: "this.addEventListener(\"enterFrame\", (): void => {\n  this.{target}.x = this.mouseX;\n  this.{target}.y = this.mouseY;\n});",
    },
    CodeSnippetDef {
        category: 0,
        title: "Enable button mode and hand cursor",
        description: "Enables the pointing hand cursor on a MovieClip and disables child mouse interception.",
        uses_target: true,
        as3_imports: &[],
        as3: "{target}.buttonMode = true;\n{target}.useHandCursor = true;\n{target}.mouseChildren = false;",
        as2: "{target}.useHandCursor = true;",
        js: "this.{target}.buttonMode = true;\nthis.{target}.useHandCursor = true;",
        ts: "this.{target}.buttonMode = true;\nthis.{target}.useHandCursor = true;",
    },
    CodeSnippetDef {
        category: 1,
        title: "Click to go to previous frame and stop",
        description: "Steps the timeline back by one frame when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onPrevFrame);\nfunction onPrevFrame(e:MouseEvent):void {\n  prevFrame();\n}",
        as2: "{target}.onRelease = function() {\n  prevFrame();\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.prevFrame();\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.prevFrame();\n});",
    },
    CodeSnippetDef {
        category: 1,
        title: "Play timeline in reverse",
        description: "Rewinds the timeline frame by frame until reaching frame 1.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "addEventListener(Event.ENTER_FRAME, onPlayReverse);\nfunction onPlayReverse(e:Event):void {\n  if (currentFrame > 1) {\n    prevFrame();\n  } else {\n    removeEventListener(Event.ENTER_FRAME, onPlayReverse);\n  }\n}",
        as2: "this.onEnterFrame = function() {\n  if (this._currentframe > 1) {\n    this.prevFrame();\n  } else {\n    delete this.onEnterFrame;\n  }\n};",
        js: "this.addEventListener(\"enterFrame\", () => {\n  if (this.currentFrame > 1) {\n    this.prevFrame();\n  }\n});",
        ts: "this.addEventListener(\"enterFrame\", (): void => {\n  if (this.currentFrame > 1) {\n    this.prevFrame();\n  }\n});",
    },
    CodeSnippetDef {
        category: 1,
        title: "Pause timeline for N seconds",
        description: "Pauses the timeline on this frame for 2 seconds before resuming playback.",
        uses_target: false,
        as3_imports: &["flash.events.TimerEvent", "flash.utils.Timer"],
        as3: "stop();\nvar pauseTimer:Timer = new Timer(2000, 1);\npauseTimer.addEventListener(TimerEvent.TIMER_COMPLETE, onResumeTimeline);\npauseTimer.start();\nfunction onResumeTimeline(e:TimerEvent):void {\n  play();\n}",
        as2: "stop();\nvar pauseId = setInterval(function() {\n  clearInterval(pauseId);\n  play();\n}, 2000);",
        js: "this.stop();\nsetTimeout(() => {\n  this.play();\n}, 2000);",
        ts: "this.stop();\nsetTimeout((): void => {\n  this.play();\n}, 2000);",
    },
    CodeSnippetDef {
        category: 1,
        title: "Jump to random frame",
        description: "Picks a random frame between 1 and totalFrames (useful for random visual variations).",
        uses_target: true,
        as3_imports: &[],
        as3: "var randomFrame:int = Math.floor(Math.random() * {target}.totalFrames) + 1;\n{target}.gotoAndStop(randomFrame);",
        as2: "var randomFrame = Math.floor(Math.random() * {target}._totalframes) + 1;\n{target}.gotoAndStop(randomFrame);",
        js: "const randomFrame = Math.floor(Math.random() * this.{target}.totalFrames) + 1;\nthis.{target}.gotoAndStop(randomFrame);",
        ts: "const randomFrame: number = Math.floor(Math.random() * this.{target}.totalFrames) + 1;\nthis.{target}.gotoAndStop(randomFrame);",
    },
    CodeSnippetDef {
        category: 2,
        title: "Fade out and hide movie clip",
        description: "Gradually reduces alpha to 0 and hides the movie clip when completely transparent.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(Event.ENTER_FRAME, onFadeOut);\nfunction onFadeOut(e:Event):void {\n  if ({target}.alpha > 0) {\n    {target}.alpha -= 0.05;\n  } else {\n    {target}.alpha = 0;\n    {target}.visible = false;\n    {target}.removeEventListener(Event.ENTER_FRAME, onFadeOut);\n  }\n}",
        as2: "{target}.onEnterFrame = function() {\n  if ({target}._alpha > 0) {\n    {target}._alpha -= 5;\n  } else {\n    {target}._alpha = 0;\n    {target}._visible = false;\n    delete {target}.onEnterFrame;\n  }\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  if (this.{target}.alpha > 0) {\n    this.{target}.alpha -= 0.05;\n  } else {\n    this.{target}.visible = false;\n  }\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  if (this.{target}.alpha > 0) {\n    this.{target}.alpha -= 0.05;\n  } else {\n    this.{target}.visible = false;\n  }\n});",
    },
    CodeSnippetDef {
        category: 2,
        title: "Pulse scale (breathing effect)",
        description: "Smoothly oscillates scaleX and scaleY using a sine wave.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var pulsePhase:Number = 0;\n{target}.addEventListener(Event.ENTER_FRAME, onPulseScale);\nfunction onPulseScale(e:Event):void {\n  pulsePhase += 0.1;\n  var s:Number = 1 + Math.sin(pulsePhase) * 0.12;\n  {target}.scaleX = s;\n  {target}.scaleY = s;\n}",
        as2: "var pulsePhase = 0;\n{target}.onEnterFrame = function() {\n  pulsePhase += 0.1;\n  var s = 100 + Math.sin(pulsePhase) * 12;\n  {target}._xscale = s;\n  {target}._yscale = s;\n};",
        js: "let pulsePhase = 0;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  pulsePhase += 0.1;\n  const s = 1 + Math.sin(pulsePhase) * 0.12;\n  this.{target}.scaleX = s;\n  this.{target}.scaleY = s;\n});",
        ts: "let pulsePhase: number = 0;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  pulsePhase += 0.1;\n  const s: number = 1 + Math.sin(pulsePhase) * 0.12;\n  this.{target}.scaleX = s;\n  this.{target}.scaleY = s;\n});",
    },
    CodeSnippetDef {
        category: 3,
        title: "Attach / duplicate movie clip dynamically",
        description: "Creates a dynamic instance on the timeline and positions it on stage.",
        uses_target: true,
        as3_imports: &["flash.display.MovieClip"],
        as3: "var spawned:MovieClip = new MovieClip();\nspawned.x = 200;\nspawned.y = 150;\naddChild(spawned);",
        as2: "var depth = this.getNextHighestDepth();\nvar spawned = {target}.duplicateMovieClip(\"clone_\" + depth, depth);\nspawned._x = 200;\nspawned._y = 150;",
        js: "const spawned = this.{target}.clone();\nspawned.x = 200;\nspawned.y = 150;\nthis.addChild(spawned);",
        ts: "const spawned = this.{target}.clone();\nspawned.x = 200;\nspawned.y = 150;\nthis.addChild(spawned);",
    },
    CodeSnippetDef {
        category: 3,
        title: "Remove all children from container",
        description: "Safely removes every child display object from the selected container clip.",
        uses_target: true,
        as3_imports: &[],
        as3: "while ({target}.numChildren > 0) {\n  {target}.removeChildAt(0);\n}",
        as2: "for (var prop in {target}) {\n  if (typeof({target}[prop]) == \"movieclip\") {\n    {target}[prop].removeMovieClip();\n  }\n}",
        js: "while (this.{target}.numChildren > 0) {\n  this.{target}.removeChildAt(0);\n}",
        ts: "while (this.{target}.numChildren > 0) {\n  this.{target}.removeChildAt(0);\n}",
    },
    CodeSnippetDef {
        category: 3,
        title: "Load external text or JSON data",
        description: "Fetches an external text or JSON file and traces the response payload.",
        uses_target: false,
        as3_imports: &[
            "flash.events.Event",
            "flash.net.URLLoader",
            "flash.net.URLRequest",
        ],
        as3: "var dataLoader:URLLoader = new URLLoader();\ndataLoader.addEventListener(Event.COMPLETE, onDataLoaded);\ndataLoader.load(new URLRequest(\"assets/config.json\"));\nfunction onDataLoaded(e:Event):void {\n  trace(dataLoader.data);\n}",
        as2: "var lv:LoadVars = new LoadVars();\nlv.onData = function(rawText) {\n  if (rawText != undefined) {\n    trace(rawText);\n  }\n};\nlv.load(\"assets/config.json\");",
        js: "fetch(\"assets/config.json\")\n  .then((res) => res.text())\n  .then((data) => console.log(data));",
        ts: "fetch(\"assets/config.json\")\n  .then((res: Response) => res.text())\n  .then((data: string) => console.log(data));",
    },
    CodeSnippetDef {
        category: 4,
        title: "Set sound volume and stereo pan",
        description: "Adjusts volume (0..1 in AS3, 0..100 in AS2) and left/right stereo panning.",
        uses_target: false,
        as3_imports: &[
            "flash.media.Sound",
            "flash.media.SoundChannel",
            "flash.media.SoundTransform",
            "flash.net.URLRequest",
        ],
        as3: "var sfx:Sound = new Sound(new URLRequest(\"assets/sfx.mp3\"));\nvar st:SoundTransform = new SoundTransform(0.75, 0.0);\nvar ch:SoundChannel = sfx.play(0, 0, st);",
        as2: "var sfx:Sound = new Sound();\nsfx.loadSound(\"assets/sfx.mp3\", true);\nsfx.setVolume(75);\nsfx.setPan(0);",
        js: "const sfx = new Audio(\"assets/sfx.mp3\");\nsfx.volume = 0.75;\nsfx.play();",
        ts: "const sfx: HTMLAudioElement = new Audio(\"assets/sfx.mp3\");\nsfx.volume = 0.75;\nsfx.play();",
    },
    CodeSnippetDef {
        category: 4,
        title: "Global mute and unmute toggle",
        description: "Toggles master audio volume between 0% and 100% when clicked.",
        uses_target: true,
        as3_imports: &[
            "flash.events.MouseEvent",
            "flash.media.SoundMixer",
            "flash.media.SoundTransform",
        ],
        as3: "var isMuted:Boolean = false;\n{target}.addEventListener(MouseEvent.CLICK, onToggleMute);\nfunction onToggleMute(e:MouseEvent):void {\n  isMuted = !isMuted;\n  SoundMixer.soundTransform = new SoundTransform(isMuted ? 0 : 1);\n}",
        as2: "var isMuted:Boolean = false;\nvar masterSound:Sound = new Sound();\n{target}.onRelease = function() {\n  isMuted = !isMuted;\n  masterSound.setVolume(isMuted ? 0 : 100);\n};",
        js: "let isMuted = false;\nthis.{target}.addEventListener(\"click\", () => {\n  isMuted = !isMuted;\n  this.setMasterVolume(isMuted ? 0 : 1);\n});",
        ts: "let isMuted: boolean = false;\nthis.{target}.addEventListener(\"click\", (): void => {\n  isMuted = !isMuted;\n  this.setMasterVolume(isMuted ? 0 : 1);\n});",
    },
    CodeSnippetDef {
        category: 5,
        title: "Mouse roll over and roll out",
        description: "Scales the selected instance up on hover and restores it when the pointer leaves.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.ROLL_OVER, onRollOverClip);\n{target}.addEventListener(MouseEvent.ROLL_OUT, onRollOutClip);\nfunction onRollOverClip(e:MouseEvent):void {\n  {target}.scaleX = 1.1;\n  {target}.scaleY = 1.1;\n}\nfunction onRollOutClip(e:MouseEvent):void {\n  {target}.scaleX = 1.0;\n  {target}.scaleY = 1.0;\n}",
        as2: "{target}.onRollOver = function() {\n  {target}._xscale = 110;\n  {target}._yscale = 110;\n};\n{target}.onRollOut = function() {\n  {target}._xscale = 100;\n  {target}._yscale = 100;\n};",
        js: "this.{target}.addEventListener(\"rollOver\", () => {\n  this.{target}.scaleX = 1.1;\n  this.{target}.scaleY = 1.1;\n});\nthis.{target}.addEventListener(\"rollOut\", () => {\n  this.{target}.scaleX = 1.0;\n  this.{target}.scaleY = 1.0;\n});",
        ts: "this.{target}.addEventListener(\"rollOver\", (): void => {\n  this.{target}.scaleX = 1.1;\n  this.{target}.scaleY = 1.1;\n});\nthis.{target}.addEventListener(\"rollOut\", (): void => {\n  this.{target}.scaleX = 1.0;\n  this.{target}.scaleY = 1.0;\n});",
    },
    CodeSnippetDef {
        category: 5,
        title: "Keyboard key down and key up",
        description: "Listens for keyboard presses on the stage and logs the key code.",
        uses_target: false,
        as3_imports: &["flash.events.KeyboardEvent"],
        as3: "stage.addEventListener(KeyboardEvent.KEY_DOWN, onKeyDown);\nfunction onKeyDown(e:KeyboardEvent):void {\n  trace(\"Key pressed: \" + e.keyCode);\n}",
        as2: "var keyListener:Object = {};\nkeyListener.onKeyDown = function() {\n  trace(\"Key pressed: \" + Key.getCode());\n};\nKey.addListener(keyListener);",
        js: "window.addEventListener(\"keydown\", (e) => {\n  console.log(\"Key pressed: \" + e.keyCode);\n});",
        ts: "window.addEventListener(\"keydown\", (e: KeyboardEvent): void => {\n  console.log(\"Key pressed: \" + e.keyCode);\n});",
    },
    CodeSnippetDef {
        category: 5,
        title: "Mouse wheel scroll handler",
        description: "Moves or zooms the selected clip when the user scrolls the mouse wheel.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "stage.addEventListener(MouseEvent.MOUSE_WHEEL, onMouseWheelScroll);\nfunction onMouseWheelScroll(e:MouseEvent):void {\n  {target}.y += e.delta * 8;\n}",
        as2: "var wheelListener:Object = {};\nwheelListener.onMouseWheel = function(delta) {\n  {target}._y += delta * 8;\n};\nMouse.addListener(wheelListener);",
        js: "window.addEventListener(\"wheel\", (e) => {\n  this.{target}.y -= Math.sign(e.deltaY) * 16;\n});",
        ts: "window.addEventListener(\"wheel\", (e: any): void => {\n  this.{target}.y -= Math.sign(e.deltaY) * 16;\n});",
    },
    CodeSnippetDef {
        category: 5,
        title: "Repeating Timer loop (Timer / setInterval)",
        description: "Executes a callback every 1000 ms using a Timer (AS3) or setInterval (AS2/JS/TS).",
        uses_target: false,
        as3_imports: &["flash.events.TimerEvent", "flash.utils.Timer"],
        as3: "var repeatTimer:Timer = new Timer(1000, 0);\nrepeatTimer.addEventListener(TimerEvent.TIMER, onTimerTick);\nrepeatTimer.start();\nfunction onTimerTick(e:TimerEvent):void {\n  trace(\"Tick: \" + repeatTimer.currentCount);\n}",
        as2: "var tickCount = 0;\nvar timerId = setInterval(function() {\n  tickCount += 1;\n  trace(\"Tick: \" + tickCount);\n}, 1000);",
        js: "let tickCount = 0;\nconst timerId = setInterval(() => {\n  tickCount += 1;\n  console.log(\"Tick: \" + tickCount);\n}, 1000);",
        ts: "let tickCount: number = 0;\nconst timerId = setInterval((): void => {\n  tickCount += 1;\n  console.log(\"Tick: \" + tickCount);\n}, 1000);",
    },
    CodeSnippetDef {
        category: 5,
        title: "Stage resize listener (centered layout)",
        description: "Keeps the selected clip centered whenever the player window is resized.",
        uses_target: true,
        as3_imports: &[
            "flash.display.StageAlign",
            "flash.display.StageScaleMode",
            "flash.events.Event",
        ],
        as3: "stage.scaleMode = StageScaleMode.NO_SCALE;\nstage.align = StageAlign.TOP_LEFT;\nstage.addEventListener(Event.RESIZE, onStageResize);\nfunction onStageResize(e:Event):void {\n  {target}.x = stage.stageWidth * 0.5;\n  {target}.y = stage.stageHeight * 0.5;\n}",
        as2: "Stage.scaleMode = \"noScale\";\nStage.align = \"TL\";\nvar resizeListener:Object = {};\nresizeListener.onResize = function() {\n  {target}._x = Stage.width * 0.5;\n  {target}._y = Stage.height * 0.5;\n};\nStage.addListener(resizeListener);",
        js: "window.addEventListener(\"resize\", () => {\n  this.{target}.x = this.stageWidth * 0.5;\n  this.{target}.y = this.stageHeight * 0.5;\n});",
        ts: "window.addEventListener(\"resize\", (): void => {\n  this.{target}.x = this.stageWidth * 0.5;\n  this.{target}.y = this.stageHeight * 0.5;\n});",
    },
    // --- 6: Math and geometry (Classic community utility functions) ---
    CodeSnippetDef {
        category: 6,
        title: "Degrees to radians and radians to degrees",
        description: "Converts angles between degrees (rotation) and radians (Math.sin, Math.cos, Math.atan2).",
        uses_target: false,
        as3_imports: &[],
        as3: "var DEG_TO_RAD:Number = Math.PI / 180;\nvar RAD_TO_DEG:Number = 180 / Math.PI;\nfunction degToRad(deg:Number):Number {\n  return deg * DEG_TO_RAD;\n}\nfunction radToDeg(rad:Number):Number {\n  return rad * RAD_TO_DEG;\n}",
        as2: "var DEG_TO_RAD = Math.PI / 180;\nvar RAD_TO_DEG = 180 / Math.PI;\nfunction degToRad(deg) {\n  return deg * DEG_TO_RAD;\n}\nfunction radToDeg(rad) {\n  return rad * RAD_TO_DEG;\n}",
        js: "const DEG_TO_RAD = Math.PI / 180;\nconst RAD_TO_DEG = 180 / Math.PI;\nfunction degToRad(deg) {\n  return deg * DEG_TO_RAD;\n}\nfunction radToDeg(rad) {\n  return rad * RAD_TO_DEG;\n}",
        ts: "const DEG_TO_RAD: number = Math.PI / 180;\nconst RAD_TO_DEG: number = 180 / Math.PI;\nfunction degToRad(deg: number): number {\n  return deg * DEG_TO_RAD;\n}\nfunction radToDeg(rad: number): number {\n  return rad * RAD_TO_DEG;\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Distance between two points (getDistance)",
        description: "Computes the Euclidean distance between (x1, y1) and (x2, y2).",
        uses_target: false,
        as3_imports: &[],
        as3: "function getDistance(x1:Number, y1:Number, x2:Number, y2:Number):Number {\n  var dx:Number = x2 - x1;\n  var dy:Number = y2 - y1;\n  return Math.sqrt(dx * dx + dy * dy);\n}",
        as2: "function getDistance(x1, y1, x2, y2) {\n  var dx = x2 - x1;\n  var dy = y2 - y1;\n  return Math.sqrt(dx * dx + dy * dy);\n}",
        js: "function getDistance(x1, y1, x2, y2) {\n  const dx = x2 - x1;\n  const dy = y2 - y1;\n  return Math.sqrt(dx * dx + dy * dy);\n}",
        ts: "function getDistance(x1: number, y1: number, x2: number, y2: number): number {\n  const dx: number = x2 - x1;\n  const dy: number = y2 - y1;\n  return Math.sqrt(dx * dx + dy * dy);\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Angle between two points in degrees (atan2)",
        description: "Returns the angle in degrees (-180..180) from point (x1, y1) to (x2, y2).",
        uses_target: false,
        as3_imports: &[],
        as3: "function getAngleDegrees(x1:Number, y1:Number, x2:Number, y2:Number):Number {\n  return Math.atan2(y2 - y1, x2 - x1) * (180 / Math.PI);\n}",
        as2: "function getAngleDegrees(x1, y1, x2, y2) {\n  return Math.atan2(y2 - y1, x2 - x1) * (180 / Math.PI);\n}",
        js: "function getAngleDegrees(x1, y1, x2, y2) {\n  return Math.atan2(y2 - y1, x2 - x1) * (180 / Math.PI);\n}",
        ts: "function getAngleDegrees(x1: number, y1: number, x2: number, y2: number): number {\n  return Math.atan2(y2 - y1, x2 - x1) * (180 / Math.PI);\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Shortest angle difference (-180 to 180)",
        description: "Normalizes the angular delta so turrets and characters turn the shortest way without wrap jumps.",
        uses_target: false,
        as3_imports: &[],
        as3: "function shortestAngleDiff(currentDeg:Number, targetDeg:Number):Number {\n  return ((targetDeg - currentDeg + 540) % 360) - 180;\n}\nfunction rotateTowards(currentDeg:Number, targetDeg:Number, factor:Number):Number {\n  return currentDeg + shortestAngleDiff(currentDeg, targetDeg) * factor;\n}",
        as2: "function shortestAngleDiff(currentDeg, targetDeg) {\n  return ((targetDeg - currentDeg + 540) % 360) - 180;\n}\nfunction rotateTowards(currentDeg, targetDeg, factor) {\n  return currentDeg + shortestAngleDiff(currentDeg, targetDeg) * factor;\n}",
        js: "function shortestAngleDiff(currentDeg, targetDeg) {\n  return ((targetDeg - currentDeg + 540) % 360) - 180;\n}\nfunction rotateTowards(currentDeg, targetDeg, factor) {\n  return currentDeg + shortestAngleDiff(currentDeg, targetDeg) * factor;\n}",
        ts: "function shortestAngleDiff(currentDeg: number, targetDeg: number): number {\n  return ((targetDeg - currentDeg + 540) % 360) - 180;\n}\nfunction rotateTowards(currentDeg: number, targetDeg: number, factor: number): number {\n  return currentDeg + shortestAngleDiff(currentDeg, targetDeg) * factor;\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Random float and integer in range",
        description: "Generates a random float in [min, max) or an inclusive random integer in [min..max].",
        uses_target: false,
        as3_imports: &[],
        as3: "function randomRange(min:Number, max:Number):Number {\n  return min + Math.random() * (max - min);\n}\nfunction randomInt(min:int, max:int):int {\n  return Math.floor(Math.random() * (max - min + 1)) + min;\n}",
        as2: "function randomRange(min, max) {\n  return min + Math.random() * (max - min);\n}\nfunction randomInt(min, max) {\n  return Math.floor(Math.random() * (max - min + 1)) + min;\n}",
        js: "function randomRange(min, max) {\n  return min + Math.random() * (max - min);\n}\nfunction randomInt(min, max) {\n  return Math.floor(Math.random() * (max - min + 1)) + min;\n}",
        ts: "function randomRange(min: number, max: number): number {\n  return min + Math.random() * (max - min);\n}\nfunction randomInt(min: number, max: number): number {\n  return Math.floor(Math.random() * (max - min + 1)) + min;\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Clamp value between min and max",
        description: "Restricts a numeric value so it never drops below min or exceeds max.",
        uses_target: false,
        as3_imports: &[],
        as3: "function clamp(val:Number, min:Number, max:Number):Number {\n  return val < min ? min : (val > max ? max : val);\n}",
        as2: "function clamp(val, min, max) {\n  return val < min ? min : (val > max ? max : val);\n}",
        js: "function clamp(val, min, max) {\n  return val < min ? min : (val > max ? max : val);\n}",
        ts: "function clamp(val: number, min: number, max: number): number {\n  return val < min ? min : (val > max ? max : val);\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Linear interpolation (lerp)",
        description: "Interpolates between start and end by factor t (0.0 to 1.0).",
        uses_target: false,
        as3_imports: &[],
        as3: "function lerp(start:Number, end:Number, t:Number):Number {\n  return start + (end - start) * t;\n}",
        as2: "function lerp(start, end, t) {\n  return start + (end - start) * t;\n}",
        js: "function lerp(start, end, t) {\n  return start + (end - start) * t;\n}",
        ts: "function lerp(start: number, end: number, t: number): number {\n  return start + (end - start) * t;\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Normalize and map value between ranges",
        description: "Normalizes a value to [0..1] or remaps it from [inMin..inMax] to [outMin..outMax].",
        uses_target: false,
        as3_imports: &[],
        as3: "function normalize(val:Number, min:Number, max:Number):Number {\n  return (val - min) / (max - min);\n}\nfunction mapRange(val:Number, inMin:Number, inMax:Number, outMin:Number, outMax:Number):Number {\n  return outMin + (outMax - outMin) * ((val - inMin) / (inMax - inMin));\n}",
        as2: "function normalize(val, min, max) {\n  return (val - min) / (max - min);\n}\nfunction mapRange(val, inMin, inMax, outMin, outMax) {\n  return outMin + (outMax - outMin) * ((val - inMin) / (inMax - inMin));\n}",
        js: "function normalize(val, min, max) {\n  return (val - min) / (max - min);\n}\nfunction mapRange(val, inMin, inMax, outMin, outMax) {\n  return outMin + (outMax - outMin) * ((val - inMin) / (inMax - inMin));\n}",
        ts: "function normalize(val: number, min: number, max: number): number {\n  return (val - min) / (max - min);\n}\nfunction mapRange(val: number, inMin: number, inMax: number, outMin: number, outMax: number): number {\n  return outMin + (outMax - outMin) * ((val - inMin) / (inMax - inMin));\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Round to decimals and snap to grid",
        description: "Rounds a float to N decimal places or snaps coordinates to a fixed grid step.",
        uses_target: false,
        as3_imports: &[],
        as3: "function roundTo(val:Number, decimals:int):Number {\n  var p:Number = Math.pow(10, decimals);\n  return Math.round(val * p) / p;\n}\nfunction snapToGrid(val:Number, step:Number):Number {\n  return Math.round(val / step) * step;\n}",
        as2: "function roundTo(val, decimals) {\n  var p = Math.pow(10, decimals);\n  return Math.round(val * p) / p;\n}\nfunction snapToGrid(val, step) {\n  return Math.round(val / step) * step;\n}",
        js: "function roundTo(val, decimals) {\n  const p = Math.pow(10, decimals);\n  return Math.round(val * p) / p;\n}\nfunction snapToGrid(val, step) {\n  return Math.round(val / step) * step;\n}",
        ts: "function roundTo(val: number, decimals: number): number {\n  const p: number = Math.pow(10, decimals);\n  return Math.round(val * p) / p;\n}\nfunction snapToGrid(val: number, step: number): number {\n  return Math.round(val / step) * step;\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Point on quadratic Bezier curve",
        description: "Evaluates (x, y) along a quadratic Bezier curve (p0 -> control p1 -> p2) at t in [0..1].",
        uses_target: false,
        as3_imports: &[],
        as3: "function bezierQuadratic(p0:Number, p1:Number, p2:Number, t:Number):Number {\n  var u:Number = 1 - t;\n  return u * u * p0 + 2 * u * t * p1 + t * t * p2;\n}",
        as2: "function bezierQuadratic(p0, p1, p2, t) {\n  var u = 1 - t;\n  return u * u * p0 + 2 * u * t * p1 + t * t * p2;\n}",
        js: "function bezierQuadratic(p0, p1, p2, t) {\n  const u = 1 - t;\n  return u * u * p0 + 2 * u * t * p1 + t * t * p2;\n}",
        ts: "function bezierQuadratic(p0: number, p1: number, p2: number, t: number): number {\n  const u: number = 1 - t;\n  return u * u * p0 + 2 * u * t * p1 + t * t * p2;\n}",
    },
    CodeSnippetDef {
        category: 6,
        title: "Polar to Cartesian offset (radius & angle)",
        description: "Converts a distance and angle in degrees into (dx, dy) stage offsets.",
        uses_target: false,
        as3_imports: &[],
        as3: "function polarToCartesian(radius:Number, angleDeg:Number):Object {\n  var rad:Number = angleDeg * (Math.PI / 180);\n  return { x: Math.cos(rad) * radius, y: Math.sin(rad) * radius };\n}",
        as2: "function polarToCartesian(radius, angleDeg) {\n  var rad = angleDeg * (Math.PI / 180);\n  return { x: Math.cos(rad) * radius, y: Math.sin(rad) * radius };\n}",
        js: "function polarToCartesian(radius, angleDeg) {\n  const rad = angleDeg * (Math.PI / 180);\n  return { x: Math.cos(rad) * radius, y: Math.sin(rad) * radius };\n}",
        ts: "function polarToCartesian(radius: number, angleDeg: number): { x: number; y: number } {\n  const rad: number = angleDeg * (Math.PI / 180);\n  return { x: Math.cos(rad) * radius, y: Math.sin(rad) * radius };\n}",
    },
    // --- 7: Physics and movement (Keith Peters Making Things Move! & game loops) ---
    CodeSnippetDef {
        category: 7,
        title: "Rotate towards mouse cursor (atan2)",
        description: "Rotates the selected movie clip every frame so it continuously aims at the mouse pointer.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(Event.ENTER_FRAME, onAimAtMouse);\nfunction onAimAtMouse(e:Event):void {\n  var dx:Number = stage.mouseX - {target}.x;\n  var dy:Number = stage.mouseY - {target}.y;\n  {target}.rotation = Math.atan2(dy, dx) * (180 / Math.PI);\n}",
        as2: "{target}.onEnterFrame = function() {\n  var dx = _root._xmouse - {target}._x;\n  var dy = _root._ymouse - {target}._y;\n  {target}._rotation = Math.atan2(dy, dx) * (180 / Math.PI);\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  const dx = this.mouseX - this.{target}.x;\n  const dy = this.mouseY - this.{target}.y;\n  this.{target}.rotation = Math.atan2(dy, dx) * (180 / Math.PI);\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  const dx: number = this.mouseX - this.{target}.x;\n  const dy: number = this.mouseY - this.{target}.y;\n  this.{target}.rotation = Math.atan2(dy, dx) * (180 / Math.PI);\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Smooth follow mouse with easing",
        description: " smoothly glides the selected movie clip towards the mouse cursor using proportional easing.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var easeFactor:Number = 0.15;\n{target}.addEventListener(Event.ENTER_FRAME, onEaseToMouse);\nfunction onEaseToMouse(e:Event):void {\n  {target}.x += (stage.mouseX - {target}.x) * easeFactor;\n  {target}.y += (stage.mouseY - {target}.y) * easeFactor;\n}",
        as2: "var easeFactor = 0.15;\n{target}.onEnterFrame = function() {\n  {target}._x += (_root._xmouse - {target}._x) * easeFactor;\n  {target}._y += (_root._ymouse - {target}._y) * easeFactor;\n};",
        js: "const easeFactor = 0.15;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  this.{target}.x += (this.mouseX - this.{target}.x) * easeFactor;\n  this.{target}.y += (this.mouseY - this.{target}.y) * easeFactor;\n});",
        ts: "const easeFactor: number = 0.15;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  this.{target}.x += (this.mouseX - this.{target}.x) * easeFactor;\n  this.{target}.y += (this.mouseY - this.{target}.y) * easeFactor;\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Elastic spring and friction follow",
        description: "Follows the mouse pointer with elastic spring acceleration and velocity damping.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var vx:Number = 0;\nvar vy:Number = 0;\nvar spring:Number = 0.12;\nvar friction:Number = 0.82;\n{target}.addEventListener(Event.ENTER_FRAME, onSpringFollow);\nfunction onSpringFollow(e:Event):void {\n  vx += (stage.mouseX - {target}.x) * spring;\n  vy += (stage.mouseY - {target}.y) * spring;\n  vx *= friction;\n  vy *= friction;\n  {target}.x += vx;\n  {target}.y += vy;\n}",
        as2: "var vx = 0;\nvar vy = 0;\nvar spring = 0.12;\nvar friction = 0.82;\n{target}.onEnterFrame = function() {\n  vx += (_root._xmouse - {target}._x) * spring;\n  vy += (_root._ymouse - {target}._y) * spring;\n  vx *= friction;\n  vy *= friction;\n  {target}._x += vx;\n  {target}._y += vy;\n};",
        js: "let vx = 0;\nlet vy = 0;\nconst spring = 0.12;\nconst friction = 0.82;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  vx = (vx + (this.mouseX - this.{target}.x) * spring) * friction;\n  vy = (vy + (this.mouseY - this.{target}.y) * spring) * friction;\n  this.{target}.x += vx;\n  this.{target}.y += vy;\n});",
        ts: "let vx: number = 0;\nlet vy: number = 0;\nconst spring: number = 0.12;\nconst friction: number = 0.82;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  vx = (vx + (this.mouseX - this.{target}.x) * spring) * friction;\n  vy = (vy + (this.mouseY - this.{target}.y) * spring) * friction;\n  this.{target}.x += vx;\n  this.{target}.y += vy;\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Move forward at current rotation angle",
        description: "Propels the selected movie clip forward along its current facing angle.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var moveSpeed:Number = 5;\n{target}.addEventListener(Event.ENTER_FRAME, onMoveForward);\nfunction onMoveForward(e:Event):void {\n  var rad:Number = {target}.rotation * (Math.PI / 180);\n  {target}.x += Math.cos(rad) * moveSpeed;\n  {target}.y += Math.sin(rad) * moveSpeed;\n}",
        as2: "var moveSpeed = 5;\n{target}.onEnterFrame = function() {\n  var rad = {target}._rotation * (Math.PI / 180);\n  {target}._x += Math.cos(rad) * moveSpeed;\n  {target}._y += Math.sin(rad) * moveSpeed;\n};",
        js: "const moveSpeed = 5;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  const rad = this.{target}.rotation * (Math.PI / 180);\n  this.{target}.x += Math.cos(rad) * moveSpeed;\n  this.{target}.y += Math.sin(rad) * moveSpeed;\n});",
        ts: "const moveSpeed: number = 5;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  const rad: number = this.{target}.rotation * (Math.PI / 180);\n  this.{target}.x += Math.cos(rad) * moveSpeed;\n  this.{target}.y += Math.sin(rad) * moveSpeed;\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "8-way keyboard movement (arrows / WASD)",
        description: "Smoothly moves the selected instance with the arrow keys using a key-state table.",
        uses_target: true,
        as3_imports: &[
            "flash.events.Event",
            "flash.events.KeyboardEvent",
            "flash.ui.Keyboard",
        ],
        as3: "var keysDown:Object = {};\nvar moveStep:Number = 5;\nstage.addEventListener(KeyboardEvent.KEY_DOWN, onKeyStateDown);\nstage.addEventListener(KeyboardEvent.KEY_UP, onKeyStateUp);\n{target}.addEventListener(Event.ENTER_FRAME, onKeyMoveStep);\nfunction onKeyStateDown(e:KeyboardEvent):void {\n  keysDown[e.keyCode] = true;\n}\nfunction onKeyStateUp(e:KeyboardEvent):void {\n  keysDown[e.keyCode] = false;\n}\nfunction onKeyMoveStep(e:Event):void {\n  if (keysDown[Keyboard.LEFT] || keysDown[65]) {target}.x -= moveStep;\n  if (keysDown[Keyboard.RIGHT] || keysDown[68]) {target}.x += moveStep;\n  if (keysDown[Keyboard.UP] || keysDown[87]) {target}.y -= moveStep;\n  if (keysDown[Keyboard.DOWN] || keysDown[83]) {target}.y += moveStep;\n}",
        as2: "var moveStep = 5;\n{target}.onEnterFrame = function() {\n  if (Key.isDown(Key.LEFT) || Key.isDown(65)) {target}._x -= moveStep;\n  if (Key.isDown(Key.RIGHT) || Key.isDown(68)) {target}._x += moveStep;\n  if (Key.isDown(Key.UP) || Key.isDown(87)) {target}._y -= moveStep;\n  if (Key.isDown(Key.DOWN) || Key.isDown(83)) {target}._y += moveStep;\n};",
        js: "const keysDown = {};\nwindow.addEventListener(\"keydown\", (e) => { keysDown[e.keyCode] = true; });\nwindow.addEventListener(\"keyup\", (e) => { keysDown[e.keyCode] = false; });\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  if (keysDown[37] || keysDown[65]) this.{target}.x -= 5;\n  if (keysDown[39] || keysDown[68]) this.{target}.x += 5;\n  if (keysDown[38] || keysDown[87]) this.{target}.y -= 5;\n  if (keysDown[40] || keysDown[83]) this.{target}.y += 5;\n});",
        ts: "const keysDown: any = {};\nwindow.addEventListener(\"keydown\", (e: KeyboardEvent): void => { keysDown[e.keyCode] = true; });\nwindow.addEventListener(\"keyup\", (e: KeyboardEvent): void => { keysDown[e.keyCode] = false; });\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  if (keysDown[37] || keysDown[65]) this.{target}.x -= 5;\n  if (keysDown[39] || keysDown[68]) this.{target}.x += 5;\n  if (keysDown[38] || keysDown[87]) this.{target}.y -= 5;\n  if (keysDown[40] || keysDown[83]) this.{target}.y += 5;\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Gravity and floor bounce",
        description: "Accelerates the selected movie clip downward with gravity and bounces off a floor Y coordinate.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var vyBounce:Number = 0;\nvar gravity:Number = 0.6;\nvar bounceFactor:Number = -0.75;\nvar floorY:Number = 360;\n{target}.addEventListener(Event.ENTER_FRAME, onGravityBounce);\nfunction onGravityBounce(e:Event):void {\n  vyBounce += gravity;\n  {target}.y += vyBounce;\n  if ({target}.y > floorY) {\n    {target}.y = floorY;\n    vyBounce *= bounceFactor;\n  }\n}",
        as2: "var vyBounce = 0;\nvar gravity = 0.6;\nvar bounceFactor = -0.75;\nvar floorY = 360;\n{target}.onEnterFrame = function() {\n  vyBounce += gravity;\n  {target}._y += vyBounce;\n  if ({target}._y > floorY) {\n    {target}._y = floorY;\n    vyBounce *= bounceFactor;\n  }\n};",
        js: "let vyBounce = 0;\nconst gravity = 0.6;\nconst bounceFactor = -0.75;\nconst floorY = 360;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  vyBounce += gravity;\n  this.{target}.y += vyBounce;\n  if (this.{target}.y > floorY) {\n    this.{target}.y = floorY;\n    vyBounce *= bounceFactor;\n  }\n});",
        ts: "let vyBounce: number = 0;\nconst gravity: number = 0.6;\nconst bounceFactor: number = -0.75;\nconst floorY: number = 360;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  vyBounce += gravity;\n  this.{target}.y += vyBounce;\n  if (this.{target}.y > floorY) {\n    this.{target}.y = floorY;\n    vyBounce *= bounceFactor;\n  }\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Circular orbit around center point",
        description: "Moves the selected movie clip along a circular orbit using Math.cos and Math.sin.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var orbitAngle:Number = 0;\nvar centerX:Number = 275;\nvar centerY:Number = 200;\nvar orbitRadius:Number = 90;\n{target}.addEventListener(Event.ENTER_FRAME, onOrbitStep);\nfunction onOrbitStep(e:Event):void {\n  orbitAngle += 0.05;\n  {target}.x = centerX + Math.cos(orbitAngle) * orbitRadius;\n  {target}.y = centerY + Math.sin(orbitAngle) * orbitRadius;\n}",
        as2: "var orbitAngle = 0;\nvar centerX = 275;\nvar centerY = 200;\nvar orbitRadius = 90;\n{target}.onEnterFrame = function() {\n  orbitAngle += 0.05;\n  {target}._x = centerX + Math.cos(orbitAngle) * orbitRadius;\n  {target}._y = centerY + Math.sin(orbitAngle) * orbitRadius;\n};",
        js: "let orbitAngle = 0;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  orbitAngle += 0.05;\n  this.{target}.x = 275 + Math.cos(orbitAngle) * 90;\n  this.{target}.y = 200 + Math.sin(orbitAngle) * 90;\n});",
        ts: "let orbitAngle: number = 0;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  orbitAngle += 0.05;\n  this.{target}.x = 275 + Math.cos(orbitAngle) * 90;\n  this.{target}.y = 200 + Math.sin(orbitAngle) * 90;\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Sine wave vertical bobbing (floating item)",
        description: "Gently floats the selected movie clip up and down around its starting Y position.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var bobBaseY:Number = {target}.y;\nvar bobAngle:Number = 0;\n{target}.addEventListener(Event.ENTER_FRAME, onSineBob);\nfunction onSineBob(e:Event):void {\n  bobAngle += 0.08;\n  {target}.y = bobBaseY + Math.sin(bobAngle) * 12;\n}",
        as2: "var bobBaseY = {target}._y;\nvar bobAngle = 0;\n{target}.onEnterFrame = function() {\n  bobAngle += 0.08;\n  {target}._y = bobBaseY + Math.sin(bobAngle) * 12;\n};",
        js: "const bobBaseY = this.{target}.y;\nlet bobAngle = 0;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  bobAngle += 0.08;\n  this.{target}.y = bobBaseY + Math.sin(bobAngle) * 12;\n});",
        ts: "const bobBaseY: number = this.{target}.y;\nlet bobAngle: number = 0;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  bobAngle += 0.08;\n  this.{target}.y = bobBaseY + Math.sin(bobAngle) * 12;\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Wrap around stage edges (screen wrap)",
        description: "Wraps the selected clip to the opposite side when it moves past the stage boundaries.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(Event.ENTER_FRAME, onWrapStageBounds);\nfunction onWrapStageBounds(e:Event):void {\n  var w:Number = stage.stageWidth;\n  var h:Number = stage.stageHeight;\n  if ({target}.x < 0) {target}.x = w;\n  else if ({target}.x > w) {target}.x = 0;\n  if ({target}.y < 0) {target}.y = h;\n  else if ({target}.y > h) {target}.y = 0;\n}",
        as2: "{target}.onEnterFrame = function() {\n  var w = Stage.width;\n  var h = Stage.height;\n  if ({target}._x < 0) {target}._x = w;\n  else if ({target}._x > w) {target}._x = 0;\n  if ({target}._y < 0) {target}._y = h;\n  else if ({target}._y > h) {target}._y = 0;\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  const w = this.stageWidth;\n  const h = this.stageHeight;\n  if (this.{target}.x < 0) this.{target}.x = w;\n  else if (this.{target}.x > w) this.{target}.x = 0;\n  if (this.{target}.y < 0) this.{target}.y = h;\n  else if (this.{target}.y > h) this.{target}.y = 0;\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  const w: number = this.stageWidth;\n  const h: number = this.stageHeight;\n  if (this.{target}.x < 0) this.{target}.x = w;\n  else if (this.{target}.x > w) this.{target}.x = 0;\n  if (this.{target}.y < 0) this.{target}.y = h;\n  else if (this.{target}.y > h) this.{target}.y = 0;\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Parallax scrolling layer",
        description: "Shifts the selected background layer proportionally to mouse displacement for depth parallax.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var parallaxOriginX:Number = {target}.x;\nvar parallaxOriginY:Number = {target}.y;\nvar parallaxDepth:Number = 0.15;\n{target}.addEventListener(Event.ENTER_FRAME, onParallaxStep);\nfunction onParallaxStep(e:Event):void {\n  var offsetX:Number = (stage.mouseX - stage.stageWidth * 0.5) * parallaxDepth;\n  var offsetY:Number = (stage.mouseY - stage.stageHeight * 0.5) * parallaxDepth;\n  {target}.x = parallaxOriginX - offsetX;\n  {target}.y = parallaxOriginY - offsetY;\n}",
        as2: "var parallaxOriginX = {target}._x;\nvar parallaxOriginY = {target}._y;\nvar parallaxDepth = 0.15;\n{target}.onEnterFrame = function() {\n  var offsetX = (_root._xmouse - Stage.width * 0.5) * parallaxDepth;\n  var offsetY = (_root._ymouse - Stage.height * 0.5) * parallaxDepth;\n  {target}._x = parallaxOriginX - offsetX;\n  {target}._y = parallaxOriginY - offsetY;\n};",
        js: "const parallaxOriginX = this.{target}.x;\nconst parallaxOriginY = this.{target}.y;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  this.{target}.x = parallaxOriginX - (this.mouseX - this.stageWidth * 0.5) * 0.15;\n  this.{target}.y = parallaxOriginY - (this.mouseY - this.stageHeight * 0.5) * 0.15;\n});",
        ts: "const parallaxOriginX: number = this.{target}.x;\nconst parallaxOriginY: number = this.{target}.y;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  this.{target}.x = parallaxOriginX - (this.mouseX - this.stageWidth * 0.5) * 0.15;\n  this.{target}.y = parallaxOriginY - (this.mouseY - this.stageHeight * 0.5) * 0.15;\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Camera / screen shake with decay",
        description: "Applies a decaying random shake offset to the selected clip or container.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var shakeOriginX:Number = {target}.x;\nvar shakeOriginY:Number = {target}.y;\nvar shakePower:Number = 12;\n{target}.addEventListener(Event.ENTER_FRAME, onShakeStep);\nfunction onShakeStep(e:Event):void {\n  if (shakePower > 0.2) {\n    {target}.x = shakeOriginX + (Math.random() * 2 - 1) * shakePower;\n    {target}.y = shakeOriginY + (Math.random() * 2 - 1) * shakePower;\n    shakePower *= 0.88;\n  } else {\n    {target}.x = shakeOriginX;\n    {target}.y = shakeOriginY;\n    {target}.removeEventListener(Event.ENTER_FRAME, onShakeStep);\n  }\n}",
        as2: "var shakeOriginX = {target}._x;\nvar shakeOriginY = {target}._y;\nvar shakePower = 12;\n{target}.onEnterFrame = function() {\n  if (shakePower > 0.2) {\n    {target}._x = shakeOriginX + (Math.random() * 2 - 1) * shakePower;\n    {target}._y = shakeOriginY + (Math.random() * 2 - 1) * shakePower;\n    shakePower *= 0.88;\n  } else {\n    {target}._x = shakeOriginX;\n    {target}._y = shakeOriginY;\n    delete {target}.onEnterFrame;\n  }\n};",
        js: "const shakeOriginX = this.{target}.x;\nconst shakeOriginY = this.{target}.y;\nlet shakePower = 12;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  if (shakePower > 0.2) {\n    this.{target}.x = shakeOriginX + (Math.random() * 2 - 1) * shakePower;\n    this.{target}.y = shakeOriginY + (Math.random() * 2 - 1) * shakePower;\n    shakePower *= 0.88;\n  } else {\n    this.{target}.x = shakeOriginX;\n    this.{target}.y = shakeOriginY;\n  }\n});",
        ts: "const shakeOriginX: number = this.{target}.x;\nconst shakeOriginY: number = this.{target}.y;\nlet shakePower: number = 12;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  if (shakePower > 0.2) {\n    this.{target}.x = shakeOriginX + (Math.random() * 2 - 1) * shakePower;\n    this.{target}.y = shakeOriginY + (Math.random() * 2 - 1) * shakePower;\n    shakePower *= 0.88;\n  } else {\n    this.{target}.x = shakeOriginX;\n    this.{target}.y = shakeOriginY;\n  }\n});",
    },
    // --- 8: Collisions and stage (Hit testing, coordinates, depth management) ---
    CodeSnippetDef {
        category: 8,
        title: "Fast circle-to-circle collision (no sqrt)",
        description: "Checks whether two circular objects overlap using squared distance (avoids Math.sqrt).",
        uses_target: false,
        as3_imports: &[],
        as3: "function hitTestCircles(x1:Number, y1:Number, r1:Number, x2:Number, y2:Number, r2:Number):Boolean {\n  var dx:Number = x2 - x1;\n  var dy:Number = y2 - y1;\n  var minDist:Number = r1 + r2;\n  return (dx * dx + dy * dy) <= (minDist * minDist);\n}",
        as2: "function hitTestCircles(x1, y1, r1, x2, y2, r2) {\n  var dx = x2 - x1;\n  var dy = y2 - y1;\n  var minDist = r1 + r2;\n  return (dx * dx + dy * dy) <= (minDist * minDist);\n}",
        js: "function hitTestCircles(x1, y1, r1, x2, y2, r2) {\n  const dx = x2 - x1;\n  const dy = y2 - y1;\n  const minDist = r1 + r2;\n  return (dx * dx + dy * dy) <= (minDist * minDist);\n}",
        ts: "function hitTestCircles(x1: number, y1: number, r1: number, x2: number, y2: number, r2: number): boolean {\n  const dx: number = x2 - x1;\n  const dy: number = y2 - y1;\n  const minDist: number = r1 + r2;\n  return (dx * dx + dy * dy) <= (minDist * minDist);\n}",
    },
    CodeSnippetDef {
        category: 8,
        title: "Bounding-box collision (hitTestObject / hitTest)",
        description: "Tests whether the selected clip's bounding box intersects another target clip.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(Event.ENTER_FRAME, onCheckBoxHit);\nfunction onCheckBoxHit(e:Event):void {\n  if ({target}.hitTestObject(this)) {\n    trace(\"Collision detected\");\n  }\n}",
        as2: "{target}.onEnterFrame = function() {\n  if ({target}.hitTest(this)) {\n    trace(\"Collision detected\");\n  }\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  if (this.{target}.hitTestObject(this)) {\n    console.log(\"Collision detected\");\n  }\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  if (this.{target}.hitTestObject(this)) {\n    console.log(\"Collision detected\");\n  }\n});",
    },
    CodeSnippetDef {
        category: 8,
        title: "Exact shape-contour hit test (shapeFlag = true)",
        description: "Checks whether the mouse pointer touches the actual vector artwork of the selected clip.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(Event.ENTER_FRAME, onCheckShapeHit);\nfunction onCheckShapeHit(e:Event):void {\n  if ({target}.hitTestPoint(stage.mouseX, stage.mouseY, true)) {\n    trace(\"Touching exact shape contour\");\n  }\n}",
        as2: "{target}.onEnterFrame = function() {\n  if ({target}.hitTest(_root._xmouse, _root._ymouse, true)) {\n    trace(\"Touching exact shape contour\");\n  }\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  if (this.{target}.hitTestPoint(this.mouseX, this.mouseY, true)) {\n    console.log(\"Touching exact shape contour\");\n  }\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  if (this.{target}.hitTestPoint(this.mouseX, this.mouseY, true)) {\n    console.log(\"Touching exact shape contour\");\n  }\n});",
    },
    CodeSnippetDef {
        category: 8,
        title: "Bring clip to front (highest depth / child index)",
        description: "Moves the selected movie clip above all sibling objects in its parent container.",
        uses_target: true,
        as3_imports: &[],
        as3: "if ({target}.parent != null) {\n  {target}.parent.setChildIndex({target}, {target}.parent.numChildren - 1);\n}",
        as2: "{target}.swapDepths({target}._parent.getNextHighestDepth());",
        js: "if (this.{target}.parent) {\n  this.{target}.parent.setChildIndex(this.{target}, this.{target}.parent.numChildren - 1);\n}",
        ts: "if (this.{target}.parent) {\n  this.{target}.parent.setChildIndex(this.{target}, this.{target}.parent.numChildren - 1);\n}",
    },
    CodeSnippetDef {
        category: 8,
        title: "Keep clip constrained inside stage bounds",
        description: "Clamps the selected movie clip's position so it cannot leave the visible stage.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(Event.ENTER_FRAME, onClampToStage);\nfunction onClampToStage(e:Event):void {\n  if ({target}.x < 0) {target}.x = 0;\n  if ({target}.x > stage.stageWidth) {target}.x = stage.stageWidth;\n  if ({target}.y < 0) {target}.y = 0;\n  if ({target}.y > stage.stageHeight) {target}.y = stage.stageHeight;\n}",
        as2: "{target}.onEnterFrame = function() {\n  if ({target}._x < 0) {target}._x = 0;\n  if ({target}._x > Stage.width) {target}._x = Stage.width;\n  if ({target}._y < 0) {target}._y = 0;\n  if ({target}._y > Stage.height) {target}._y = Stage.height;\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  this.{target}.x = Math.max(0, Math.min(this.stageWidth, this.{target}.x));\n  this.{target}.y = Math.max(0, Math.min(this.stageHeight, this.{target}.y));\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  this.{target}.x = Math.max(0, Math.min(this.stageWidth, this.{target}.x));\n  this.{target}.y = Math.max(0, Math.min(this.stageHeight, this.{target}.y));\n});",
    },
    CodeSnippetDef {
        category: 8,
        title: "Convert local to global stage coordinates",
        description: "Translates the selected clip's local (0, 0) registration point into global stage coordinates.",
        uses_target: true,
        as3_imports: &["flash.geom.Point"],
        as3: "var stagePt:Point = {target}.localToGlobal(new Point(0, 0));\ntrace(\"Stage X: \" + stagePt.x + \", Y: \" + stagePt.y);",
        as2: "var stagePt:Object = { x: 0, y: 0 };\n{target}.localToGlobal(stagePt);\ntrace(\"Stage X: \" + stagePt.x + \", Y: \" + stagePt.y);",
        js: "const stagePt = this.{target}.localToGlobal({ x: 0, y: 0 });\nconsole.log(\"Stage X: \" + stagePt.x + \", Y: \" + stagePt.y);",
        ts: "const stagePt = this.{target}.localToGlobal({ x: 0, y: 0 });\nconsole.log(\"Stage X: \" + stagePt.x + \", Y: \" + stagePt.y);",
    },
    // --- 9: Arrays, strings and color (Proto.Layer51, Senocular & Robert Penner gems) ---
    CodeSnippetDef {
        category: 9,
        title: "Shuffle array in-place (Fisher-Yates)",
        description: "Randomizes the order of elements in an array with uniform probability.",
        uses_target: false,
        as3_imports: &[],
        as3: "function shuffleArray(arr:Array):Array {\n  var i:int = arr.length;\n  while (--i > 0) {\n    var j:int = Math.floor(Math.random() * (i + 1));\n    var tmp:* = arr[i];\n    arr[i] = arr[j];\n    arr[j] = tmp;\n  }\n  return arr;\n}",
        as2: "function shuffleArray(arr) {\n  var i = arr.length;\n  while (--i > 0) {\n    var j = Math.floor(Math.random() * (i + 1));\n    var tmp = arr[i];\n    arr[i] = arr[j];\n    arr[j] = tmp;\n  }\n  return arr;\n}",
        js: "function shuffleArray(arr) {\n  let i = arr.length;\n  while (--i > 0) {\n    const j = Math.floor(Math.random() * (i + 1));\n    const tmp = arr[i];\n    arr[i] = arr[j];\n    arr[j] = tmp;\n  }\n  return arr;\n}",
        ts: "function shuffleArray(arr: any[]): any[] {\n  let i: number = arr.length;\n  while (--i > 0) {\n    const j: number = Math.floor(Math.random() * (i + 1));\n    const tmp: any = arr[i];\n    arr[i] = arr[j];\n    arr[j] = tmp;\n  }\n  return arr;\n}",
    },
    CodeSnippetDef {
        category: 9,
        title: "Pick random item from array",
        description: "Returns a randomly chosen element from any non-empty array.",
        uses_target: false,
        as3_imports: &[],
        as3: "function randomItem(arr:Array):* {\n  return arr[Math.floor(Math.random() * arr.length)];\n}",
        as2: "function randomItem(arr) {\n  return arr[Math.floor(Math.random() * arr.length)];\n}",
        js: "function randomItem(arr) {\n  return arr[Math.floor(Math.random() * arr.length)];\n}",
        ts: "function randomItem(arr: any[]): any {\n  return arr[Math.floor(Math.random() * arr.length)];\n}",
    },
    CodeSnippetDef {
        category: 9,
        title: "Remove item from array (reverse loop & splice)",
        description: "Removes matching elements safely by iterating backwards so splice never skips an index.",
        uses_target: false,
        as3_imports: &[],
        as3: "function removeValueFromArray(arr:Array, targetVal:*):Array {\n  for (var i:int = arr.length - 1; i >= 0; i--) {\n    if (arr[i] === targetVal) {\n      arr.splice(i, 1);\n    }\n  }\n  return arr;\n}",
        as2: "function removeValueFromArray(arr, targetVal) {\n  for (var i = arr.length - 1; i >= 0; i--) {\n    if (arr[i] == targetVal) {\n      arr.splice(i, 1);\n    }\n  }\n  return arr;\n}",
        js: "function removeValueFromArray(arr, targetVal) {\n  for (let i = arr.length - 1; i >= 0; i--) {\n    if (arr[i] === targetVal) {\n      arr.splice(i, 1);\n    }\n  }\n  return arr;\n}",
        ts: "function removeValueFromArray(arr: any[], targetVal: any): any[] {\n  for (let i: number = arr.length - 1; i >= 0; i--) {\n    if (arr[i] === targetVal) {\n      arr.splice(i, 1);\n    }\n  }\n  return arr;\n}",
    },
    CodeSnippetDef {
        category: 9,
        title: "Remove duplicates from array (unique)",
        description: "Returns a new array containing only the first occurrence of each element.",
        uses_target: false,
        as3_imports: &[],
        as3: "function uniqueArray(arr:Array):Array {\n  var out:Array = [];\n  for (var i:int = 0; i < arr.length; i++) {\n    if (out.indexOf(arr[i]) == -1) {\n      out.push(arr[i]);\n    }\n  }\n  return out;\n}",
        as2: "function uniqueArray(arr) {\n  var out = [];\n  for (var i = 0; i < arr.length; i++) {\n    var found = false;\n    for (var j = 0; j < out.length; j++) {\n      if (out[j] == arr[i]) { found = true; break; }\n    }\n    if (!found) out.push(arr[i]);\n  }\n  return out;\n}",
        js: "function uniqueArray(arr) {\n  return Array.from(new Set(arr));\n}",
        ts: "function uniqueArray(arr: any[]): any[] {\n  return Array.from(new Set(arr));\n}",
    },
    CodeSnippetDef {
        category: 9,
        title: "Convert RGB to Hex and Hex to RGB",
        description: "Combines 0..255 R, G, B channels into a 0xRRGGBB integer and extracts them back.",
        uses_target: false,
        as3_imports: &[],
        as3: "function rgbToHex(r:uint, g:uint, b:uint):uint {\n  return ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF);\n}\nfunction hexToRgb(hex:uint):Object {\n  return {\n    r: (hex >> 16) & 0xFF,\n    g: (hex >> 8) & 0xFF,\n    b: hex & 0xFF\n  };\n}",
        as2: "function rgbToHex(r, g, b) {\n  return ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF);\n}\nfunction hexToRgb(hex) {\n  return {\n    r: (hex >> 16) & 0xFF,\n    g: (hex >> 8) & 0xFF,\n    b: hex & 0xFF\n  };\n}",
        js: "function rgbToHex(r, g, b) {\n  return ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF);\n}\nfunction hexToRgb(hex) {\n  return {\n    r: (hex >> 16) & 0xFF,\n    g: (hex >> 8) & 0xFF,\n    b: hex & 0xFF\n  };\n}",
        ts: "function rgbToHex(r: number, g: number, b: number): number {\n  return ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF);\n}\nfunction hexToRgb(hex: number): { r: number; g: number; b: number } {\n  return {\n    r: (hex >> 16) & 0xFF,\n    g: (hex >> 8) & 0xFF,\n    b: hex & 0xFF\n  };\n}",
    },
    CodeSnippetDef {
        category: 9,
        title: "Blend / interpolate two Hex colors",
        description: "Smoothly blends between two 0xRRGGBB colors by factor t in [0.0..1.0].",
        uses_target: false,
        as3_imports: &[],
        as3: "function interpolateColor(c1:uint, c2:uint, t:Number):uint {\n  var r1:uint = (c1 >> 16) & 0xFF;\n  var g1:uint = (c1 >> 8) & 0xFF;\n  var b1:uint = c1 & 0xFF;\n  var r:uint = uint(Math.round(r1 + (((c2 >> 16) & 0xFF) - r1) * t));\n  var g:uint = uint(Math.round(g1 + (((c2 >> 8) & 0xFF) - g1) * t));\n  var b:uint = uint(Math.round(b1 + ((c2 & 0xFF) - b1) * t));\n  return (r << 16) | (g << 8) | b;\n}",
        as2: "function interpolateColor(c1, c2, t) {\n  var r1 = (c1 >> 16) & 0xFF;\n  var g1 = (c1 >> 8) & 0xFF;\n  var b1 = c1 & 0xFF;\n  var r = Math.round(r1 + (((c2 >> 16) & 0xFF) - r1) * t);\n  var g = Math.round(g1 + (((c2 >> 8) & 0xFF) - g1) * t);\n  var b = Math.round(b1 + ((c2 & 0xFF) - b1) * t);\n  return (r << 16) | (g << 8) | b;\n}",
        js: "function interpolateColor(c1, c2, t) {\n  const r1 = (c1 >> 16) & 0xFF;\n  const g1 = (c1 >> 8) & 0xFF;\n  const b1 = c1 & 0xFF;\n  const r = Math.round(r1 + (((c2 >> 16) & 0xFF) - r1) * t);\n  const g = Math.round(g1 + (((c2 >> 8) & 0xFF) - g1) * t);\n  const b = Math.round(b1 + ((c2 & 0xFF) - b1) * t);\n  return (r << 16) | (g << 8) | b;\n}",
        ts: "function interpolateColor(c1: number, c2: number, t: number): number {\n  const r1: number = (c1 >> 16) & 0xFF;\n  const g1: number = (c1 >> 8) & 0xFF;\n  const b1: number = c1 & 0xFF;\n  const r: number = Math.round(r1 + (((c2 >> 16) & 0xFF) - r1) * t);\n  const g: number = Math.round(g1 + (((c2 >> 8) & 0xFF) - g1) * t);\n  const b: number = Math.round(b1 + ((c2 & 0xFF) - b1) * t);\n  return (r << 16) | (g << 8) | b;\n}",
    },
    CodeSnippetDef {
        category: 9,
        title: "Tint movie clip with Hex color",
        description: "Applies a solid RGB color tint to the selected movie clip.",
        uses_target: true,
        as3_imports: &["flash.geom.ColorTransform"],
        as3: "var tintTransform:ColorTransform = {target}.transform.colorTransform;\ntintTransform.color = 0xFF5500;\n{target}.transform.colorTransform = tintTransform;",
        as2: "var clipColor:Color = new Color({target});\nclipColor.setRGB(0xFF5500);",
        js: "this.{target}.tint = 0xFF5500;",
        ts: "this.{target}.tint = 0xFF5500;",
    },
    CodeSnippetDef {
        category: 9,
        title: "Format seconds as MM:SS timer (padZero)",
        description: "Formats a duration in seconds as a zero-padded MM:SS timer string.",
        uses_target: false,
        as3_imports: &[],
        as3: "function padZero(n:int):String {\n  return (n < 10 ? \"0\" : \"\") + n;\n}\nfunction formatTimeMMSS(totalSeconds:Number):String {\n  var mins:int = int(Math.floor(totalSeconds / 60));\n  var secs:int = int(Math.floor(totalSeconds % 60));\n  return padZero(mins) + \":\" + padZero(secs);\n}",
        as2: "function padZero(n) {\n  return (n < 10 ? \"0\" : \"\") + n;\n}\nfunction formatTimeMMSS(totalSeconds) {\n  var mins = Math.floor(totalSeconds / 60);\n  var secs = Math.floor(totalSeconds % 60);\n  return padZero(mins) + \":\" + padZero(secs);\n}",
        js: "function padZero(n) {\n  return (n < 10 ? \"0\" : \"\") + n;\n}\nfunction formatTimeMMSS(totalSeconds) {\n  const mins = Math.floor(totalSeconds / 60);\n  const secs = Math.floor(totalSeconds % 60);\n  return padZero(mins) + \":\" + padZero(secs);\n}",
        ts: "function padZero(n: number): string {\n  return (n < 10 ? \"0\" : \"\") + n;\n}\nfunction formatTimeMMSS(totalSeconds: number): string {\n  const mins: number = Math.floor(totalSeconds / 60);\n  const secs: number = Math.floor(totalSeconds % 60);\n  return padZero(mins) + \":\" + padZero(secs);\n}",
    },
    CodeSnippetDef {
        category: 9,
        title: "Format score with thousands commas",
        description: "Formats an integer score with comma separators (e.g. 1234567 -> \"1,234,567\").",
        uses_target: false,
        as3_imports: &[],
        as3: "function formatScore(score:Number):String {\n  var s:String = String(Math.floor(score));\n  var out:String = \"\";\n  while (s.length > 3) {\n    out = \",\" + s.substr(s.length - 3) + out;\n    s = s.substr(0, s.length - 3);\n  }\n  return s + out;\n}",
        as2: "function formatScore(score) {\n  var s = String(Math.floor(score));\n  var out = \"\";\n  while (s.length > 3) {\n    out = \",\" + s.substr(s.length - 3) + out;\n    s = s.substr(0, s.length - 3);\n  }\n  return s + out;\n}",
        js: "function formatScore(score) {\n  let s = String(Math.floor(score));\n  let out = \"\";\n  while (s.length > 3) {\n    out = \",\" + s.slice(-3) + out;\n    s = s.slice(0, -3);\n  }\n  return s + out;\n}",
        ts: "function formatScore(score: number): string {\n  let s: string = String(Math.floor(score));\n  let out: string = \"\";\n  while (s.length > 3) {\n    out = \",\" + s.slice(-3) + out;\n    s = s.slice(0, -3);\n  }\n  return s + out;\n}",
    },
    CodeSnippetDef {
        category: 9,
        title: "Trim whitespace and replace all substrings",
        description: "Classic RegExp-free string helpers for trimming whitespace and replacing all occurrences.",
        uses_target: false,
        as3_imports: &[],
        as3: "function replaceAll(str:String, search:String, replacement:String):String {\n  return str.split(search).join(replacement);\n}\nfunction trimString(str:String):String {\n  var s:int = 0;\n  var e:int = str.length - 1;\n  while (s <= e && str.charCodeAt(s) <= 32) s++;\n  while (e >= s && str.charCodeAt(e) <= 32) e--;\n  return str.substring(s, e + 1);\n}",
        as2: "function replaceAll(str, search, replacement) {\n  return str.split(search).join(replacement);\n}\nfunction trimString(str) {\n  var s = 0;\n  var e = str.length - 1;\n  while (s <= e && str.charCodeAt(s) <= 32) s++;\n  while (e >= s && str.charCodeAt(e) <= 32) e--;\n  return str.substring(s, e + 1);\n}",
        js: "function replaceAll(str, search, replacement) {\n  return str.split(search).join(replacement);\n}\nfunction trimString(str) {\n  return str.trim();\n}",
        ts: "function replaceAll(str: string, search: string, replacement: string): string {\n  return str.split(search).join(replacement);\n}\nfunction trimString(str: string): string {\n  return str.trim();\n}",
    },
    // --- 10: Preloaders, storage and debug ---
    CodeSnippetDef {
        category: 10,
        title: "Save and load local data (SharedObject)",
        description: "Persists high scores or user settings locally using Flash SharedObject / localStorage.",
        uses_target: false,
        as3_imports: &["flash.net.SharedObject"],
        as3: "var saveStore:SharedObject = SharedObject.getLocal(\"neoflash_save\");\nsaveStore.data.highScore = 1000;\nsaveStore.flush();\ntrace(\"Saved high score: \" + saveStore.data.highScore);",
        as2: "var saveStore:SharedObject = SharedObject.getLocal(\"neoflash_save\");\nsaveStore.data.highScore = 1000;\nsaveStore.flush();\ntrace(\"Saved high score: \" + saveStore.data.highScore);",
        js: "window.localStorage.setItem(\"neoflash_highScore\", \"1000\");\nconsole.log(\"Saved high score: \" + window.localStorage.getItem(\"neoflash_highScore\"));",
        ts: "window.localStorage.setItem(\"neoflash_highScore\", \"1000\");\nconsole.log(\"Saved high score: \" + window.localStorage.getItem(\"neoflash_highScore\"));",
    },
    CodeSnippetDef {
        category: 10,
        title: "Preloader progress bar (bytesLoaded / bytesTotal)",
        description: "Scales the selected progress-bar clip from 0% to 100% while the movie loads, then advances.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "stop();\naddEventListener(Event.ENTER_FRAME, onPreloadTick);\nfunction onPreloadTick(e:Event):void {\n  var loaded:Number = loaderInfo.bytesLoaded;\n  var total:Number = loaderInfo.bytesTotal;\n  var ratio:Number = total > 0 ? (loaded / total) : 1;\n  {target}.scaleX = ratio;\n  if (ratio >= 1) {\n    removeEventListener(Event.ENTER_FRAME, onPreloadTick);\n    gotoAndPlay(2);\n  }\n}",
        as2: "stop();\nthis.onEnterFrame = function() {\n  var loaded = _root.getBytesLoaded();\n  var total = _root.getBytesTotal();\n  var pct = total > 0 ? Math.floor((loaded / total) * 100) : 100;\n  {target}._xscale = pct;\n  if (pct >= 100) {\n    delete this.onEnterFrame;\n    gotoAndPlay(2);\n  }\n};",
        js: "this.stop();\nthis.{target}.scaleX = 1;\nthis.gotoAndPlay(2);",
        ts: "this.stop();\nthis.{target}.scaleX = 1;\nthis.gotoAndPlay(2);",
    },
    CodeSnippetDef {
        category: 10,
        title: "FPS and delta-time counter (getTimer)",
        description: "Measures frame-to-frame delta time in seconds and live frames-per-second using getTimer().",
        uses_target: false,
        as3_imports: &["flash.events.Event", "flash.utils.getTimer"],
        as3: "var lastTickMs:int = getTimer();\nvar frameCount:int = 0;\nvar elapsedAccMs:int = 0;\naddEventListener(Event.ENTER_FRAME, onMeasureFps);\nfunction onMeasureFps(e:Event):void {\n  var nowMs:int = getTimer();\n  var dtMs:int = nowMs - lastTickMs;\n  lastTickMs = nowMs;\n  frameCount += 1;\n  elapsedAccMs += dtMs;\n  if (elapsedAccMs >= 1000) {\n    trace(\"FPS: \" + frameCount);\n    frameCount = 0;\n    elapsedAccMs -= 1000;\n  }\n}",
        as2: "var lastTickMs = getTimer();\nvar frameCount = 0;\nvar elapsedAccMs = 0;\nthis.onEnterFrame = function() {\n  var nowMs = getTimer();\n  var dtMs = nowMs - lastTickMs;\n  lastTickMs = nowMs;\n  frameCount += 1;\n  elapsedAccMs += dtMs;\n  if (elapsedAccMs >= 1000) {\n    trace(\"FPS: \" + frameCount);\n    frameCount = 0;\n    elapsedAccMs -= 1000;\n  }\n};",
        js: "let lastTickMs = Date.now();\nlet frameCount = 0;\nlet elapsedAccMs = 0;\nthis.addEventListener(\"enterFrame\", () => {\n  const nowMs = Date.now();\n  const dtMs = nowMs - lastTickMs;\n  lastTickMs = nowMs;\n  frameCount += 1;\n  elapsedAccMs += dtMs;\n  if (elapsedAccMs >= 1000) {\n    console.log(\"FPS: \" + frameCount);\n    frameCount = 0;\n    elapsedAccMs -= 1000;\n  }\n});",
        ts: "let lastTickMs: number = Date.now();\nlet frameCount: number = 0;\nlet elapsedAccMs: number = 0;\nthis.addEventListener(\"enterFrame\", (): void => {\n  const nowMs: number = Date.now();\n  const dtMs: number = nowMs - lastTickMs;\n  lastTickMs = nowMs;\n  frameCount += 1;\n  elapsedAccMs += dtMs;\n  if (elapsedAccMs >= 1000) {\n    console.log(\"FPS: \" + frameCount);\n    frameCount = 0;\n    elapsedAccMs -= 1000;\n  }\n});",
    },
    CodeSnippetDef {
        category: 5,
        title: "Double-click handler (doubleClickEnabled / getTimer)",
        description: "Detects a rapid double-click on the selected movie clip across AS3, AS2, JS and TS.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.doubleClickEnabled = true;\n{target}.addEventListener(MouseEvent.DOUBLE_CLICK, onDoubleClickClip);\nfunction onDoubleClickClip(e:MouseEvent):void {\n  trace(\"Double-clicked\");\n}",
        as2: "var lastClickMs = 0;\n{target}.onPress = function() {\n  var nowMs = getTimer();\n  if (nowMs - lastClickMs < 300) {\n    trace(\"Double-clicked\");\n  }\n  lastClickMs = nowMs;\n};",
        js: "this.{target}.addEventListener(\"dblclick\", () => {\n  console.log(\"Double-clicked\");\n});",
        ts: "this.{target}.addEventListener(\"dblclick\", (): void => {\n  console.log(\"Double-clicked\");\n});",
    },
    CodeSnippetDef {
        category: 7,
        title: "Smooth turret turn towards mouse (shortest angle)",
        description: "Smoothly rotates the selected clip towards the pointer along the shortest arc (-180..180).",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "var turnEase:Number = 0.15;\n{target}.addEventListener(Event.ENTER_FRAME, onSmoothTurnToMouse);\nfunction onSmoothTurnToMouse(e:Event):void {\n  var targetDeg:Number = Math.atan2(stage.mouseY - {target}.y, stage.mouseX - {target}.x) * (180 / Math.PI);\n  var diff:Number = ((targetDeg - {target}.rotation + 540) % 360) - 180;\n  {target}.rotation += diff * turnEase;\n}",
        as2: "var turnEase = 0.15;\n{target}.onEnterFrame = function() {\n  var targetDeg = Math.atan2(_root._ymouse - {target}._y, _root._xmouse - {target}._x) * (180 / Math.PI);\n  var diff = ((targetDeg - {target}._rotation + 540) % 360) - 180;\n  {target}._rotation += diff * turnEase;\n};",
        js: "const turnEase = 0.15;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  const targetDeg = Math.atan2(this.mouseY - this.{target}.y, this.mouseX - this.{target}.x) * (180 / Math.PI);\n  const diff = ((targetDeg - this.{target}.rotation + 540) % 360) - 180;\n  this.{target}.rotation += diff * turnEase;\n});",
        ts: "const turnEase: number = 0.15;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  const targetDeg: number = Math.atan2(this.mouseY - this.{target}.y, this.mouseX - this.{target}.x) * (180 / Math.PI);\n  const diff: number = ((targetDeg - this.{target}.rotation + 540) % 360) - 180;\n  this.{target}.rotation += diff * turnEase;\n});",
    },
    CodeSnippetDef {
        category: 9,
        title: "Random 24-bit Hex color generator",
        description: "Generates a random 0xRRGGBB color integer.",
        uses_target: false,
        as3_imports: &[],
        as3: "function randomColorHex():uint {\n  return uint(Math.floor(Math.random() * 0xFFFFFF));\n}",
        as2: "function randomColorHex() {\n  return Math.floor(Math.random() * 0xFFFFFF);\n}",
        js: "function randomColorHex() {\n  return Math.floor(Math.random() * 0xFFFFFF);\n}",
        ts: "function randomColorHex(): number {\n  return Math.floor(Math.random() * 0xFFFFFF);\n}",
    },
    CodeSnippetDef {
        category: 10,
        title: "Countdown timer from N seconds to zero",
        description: "Counts down once per second and runs completion logic when remaining seconds reach 0.",
        uses_target: false,
        as3_imports: &["flash.events.TimerEvent", "flash.utils.Timer"],
        as3: "var remainingSeconds:int = 60;\nvar countdownTimer:Timer = new Timer(1000, remainingSeconds);\ncountdownTimer.addEventListener(TimerEvent.TIMER, onCountdownStep);\ncountdownTimer.addEventListener(TimerEvent.TIMER_COMPLETE, onCountdownDone);\ncountdownTimer.start();\nfunction onCountdownStep(e:TimerEvent):void {\n  remainingSeconds -= 1;\n  trace(\"Time left: \" + remainingSeconds);\n}\nfunction onCountdownDone(e:TimerEvent):void {\n  trace(\"Time is up!\");\n}",
        as2: "var remainingSeconds = 60;\nvar countdownId = setInterval(function() {\n  remainingSeconds -= 1;\n  trace(\"Time left: \" + remainingSeconds);\n  if (remainingSeconds <= 0) {\n    clearInterval(countdownId);\n    trace(\"Time is up!\");\n  }\n}, 1000);",
        js: "let remainingSeconds = 60;\nlet countdownId = 0;\ncountdownId = setInterval(() => {\n  remainingSeconds -= 1;\n  console.log(\"Time left: \" + remainingSeconds);\n  if (remainingSeconds <= 0) {\n    clearInterval(countdownId);\n    console.log(\"Time is up!\");\n  }\n}, 1000);",
        ts: "let remainingSeconds: number = 60;\nlet countdownId: any = 0;\ncountdownId = setInterval((): void => {\n  remainingSeconds -= 1;\n  console.log(\"Time left: \" + remainingSeconds);\n  if (remainingSeconds <= 0) {\n    clearInterval(countdownId);\n    console.log(\"Time is up!\");\n  }\n}, 1000);",
    },
    CodeSnippetDef {
        category: 10,
        title: "Custom right-click ContextMenu item",
        description: "Hides built-in Flash context menu items and attaches a custom right-click menu entry.",
        uses_target: false,
        as3_imports: &[
            "flash.events.ContextMenuEvent",
            "flash.ui.ContextMenu",
            "flash.ui.ContextMenuItem",
        ],
        as3: "var customMenu:ContextMenu = new ContextMenu();\ncustomMenu.hideBuiltInItems();\nvar aboutItem:ContextMenuItem = new ContextMenuItem(\"Built with NeoFlash\");\naboutItem.addEventListener(ContextMenuEvent.MENU_ITEM_SELECT, onSelectAboutItem);\ncustomMenu.customItems.push(aboutItem);\nthis.contextMenu = customMenu;\nfunction onSelectAboutItem(e:ContextMenuEvent):void {\n  trace(\"Custom context menu item selected\");\n}",
        as2: "var customMenu:ContextMenu = new ContextMenu();\ncustomMenu.hideBuiltInItems();\ncustomMenu.customItems.push(new ContextMenuItem(\"Built with NeoFlash\", function() {\n  trace(\"Custom context menu item selected\");\n}));\n_root.menu = customMenu;",
        js: "window.addEventListener(\"contextmenu\", (e) => {\n  e.preventDefault();\n  console.log(\"Custom context menu item selected\");\n});",
        ts: "window.addEventListener(\"contextmenu\", (e: MouseEvent): void => {\n  e.preventDefault();\n  console.log(\"Custom context menu item selected\");\n});",
    },
];

impl CodeSnippetDef {
    pub fn render_body(&self, language: ScriptLanguage, target: &str) -> String {
        let template = match language {
            ScriptLanguage::ActionScript3 => self.as3,
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => self.as2,
            ScriptLanguage::JavaScript => self.js,
            ScriptLanguage::TypeScript => self.ts,
        };
        template.replace("{target}", target)
    }

    /// Combine `existing` frame script source with this snippet, adding any missing AS3 imports
    /// to the leading import block.
    pub fn merge_into(&self, existing: &str, language: ScriptLanguage, target: &str) -> String {
        let body = self.render_body(language, target);
        let mut out = existing.to_owned();
        if language == ScriptLanguage::ActionScript3 {
            for qualified in self.as3_imports {
                if let Some(line) = crate::script_api::missing_import(&out, qualified) {
                    let at = out
                        .match_indices("import ")
                        .filter(|(i, _)| *i == 0 || out.as_bytes()[i - 1] == b'\n')
                        .last()
                        .map_or(0, |(i, _)| {
                            out[i..].find('\n').map_or(out.len(), |n| i + n + 1)
                        });
                    let formatted = if at == out.len() && !out.ends_with('\n') && !out.is_empty() {
                        format!("\n{line}\n")
                    } else {
                        format!("{line}\n")
                    };
                    out.insert_str(at, &formatted);
                }
            }
        }
        if out.trim().is_empty() {
            body
        } else if out.ends_with("\n\n")
            || out.lines().all(|l| l.trim_start().starts_with("import "))
        {
            if out.ends_with('\n') {
                format!("{out}{body}")
            } else {
                format!("{out}\n{body}")
            }
        } else if out.ends_with('\n') {
            format!("{out}\n{body}")
        } else {
            format!("{out}\n\n{body}")
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CodeSnippetsState {
    pub(crate) query: String,
    pub(crate) open_categories: [bool; SNIPPET_CATEGORIES.len()],
    pub(crate) selected: usize,
}

impl Default for CodeSnippetsState {
    fn default() -> Self {
        let mut open_categories = [false; SNIPPET_CATEGORIES.len()];
        // Matches NF2 reference (`Actions` and `Animation` open, `"Rotate continuously"` selected).
        open_categories[0] = true;
        open_categories[2] = true;
        Self {
            query: String::new(),
            open_categories,
            selected: DEFAULT_SNIPPET_INDEX,
        }
    }
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn sanitize_instance_stem(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if (ch == ' ' || ch == '-') && !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "orbit".to_owned()
    } else if trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("clip_{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

impl Editor {
    pub(crate) fn active_snippet_language(&self) -> ScriptLanguage {
        if let Some(fs) = self
            .symbol()
            .timeline
            .scripts
            .iter()
            .find(|fs| fs.frame == self.frame)
        {
            return fs.script.language;
        }
        if let Some(unit) = self.project.sources.first() {
            return unit.language;
        }
        for sym in &self.project.symbols {
            if let Some(fs) = sym.timeline.scripts.first() {
                return fs.script.language;
            }
            for layer in &sym.timeline.layers {
                for key in &layer.keyframes {
                    for el in &key.elements {
                        if let Some(os) = el.scripts.first() {
                            return os.script.language;
                        }
                    }
                }
            }
        }
        ScriptLanguage::ActionScript3
    }

    /// Returns `(target_identifier, Some((element_id, auto_assigned_name)))` when a selected
    /// unnamed symbol instance should receive an automatic instance name upon snippet insertion.
    fn snippet_target_instance(&self) -> (String, Option<(String, String)>) {
        if let Some(el) = self.selected_element() {
            let trimmed = el.name.trim();
            if is_valid_ident(trimmed) {
                return (trimmed.to_owned(), None);
            }
            if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                let stem = self
                    .project
                    .symbol(symbol_id)
                    .map_or_else(|| "orbit".to_owned(), |s| sanitize_instance_stem(&s.name));
                let suffix = match self
                    .project
                    .symbol(symbol_id)
                    .map(|s| s.kind)
                    .unwrap_or(SymbolKind::MovieClip)
                {
                    SymbolKind::Button => "_btn",
                    SymbolKind::MovieClip | SymbolKind::Graphic => "_mc",
                };
                let base = if stem.ends_with("_mc") || stem.ends_with("_btn") {
                    stem
                } else {
                    format!("{stem}{suffix}")
                };
                let existing: std::collections::HashSet<&str> = self
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .filter_map(|l| l.key_at(self.frame))
                    .flat_map(|k| &k.elements)
                    .map(|e| e.name.as_str())
                    .filter(|n| !n.is_empty())
                    .collect();
                let mut candidate = base.clone();
                let mut n = 2usize;
                while existing.contains(candidate.as_str()) {
                    candidate = format!("{base}_{n}");
                    n += 1;
                }
                return (candidate.clone(), Some((el.id.clone(), candidate)));
            }
        }
        ("orbit_mc".to_owned(), None)
    }

    pub fn insert_code_snippet(&mut self, snippet_index: usize) -> bool {
        if self.symbol().kind == SymbolKind::Button {
            return false;
        }
        let Some(snippet) = CODE_SNIPPETS.get(snippet_index) else {
            return false;
        };
        let language = self.active_snippet_language();
        let (target, auto_name) = if snippet.uses_target {
            self.snippet_target_instance()
        } else {
            ("orbit_mc".to_owned(), None)
        };
        self.finish_edits();
        if self.playing {
            self.set_playing(false);
        }
        self.transact_named("Insert code snippet", |s| {
            if let Some((el_id, new_name)) = auto_name
                && s.selected_editable()
            {
                s.ensure_selected_key();
                if let Some(el) = s.selected_element_mut().filter(|e| e.id == el_id) {
                    el.name = new_name;
                }
            }
            let frame = s.frame;
            if frame >= s.symbol().timeline.frame_count {
                s.ensure_key(false);
            }
            let existing = s
                .symbol()
                .timeline
                .scripts
                .iter()
                .find(|fs| fs.frame == frame)
                .map_or("", |fs| fs.script.source.as_str())
                .to_owned();
            let merged = snippet.merge_into(&existing, language, &target);
            let scripts = &mut s.symbol_mut().timeline.scripts;
            scripts.retain(|fs| fs.frame != frame);
            scripts.push(FrameScript {
                frame,
                script: Script {
                    language,
                    source: merged,
                },
            });
        });
        self.script_target = ScriptTarget::Frame;
        self.open_actions();
        true
    }

    pub(crate) fn code_snippets_panel_ui(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        ui.spacing_mut().item_spacing.y = space::S1;

        let w = ui.available_width();
        ui.add(c::Search::new(&mut self.code_snippets.query, "Search snippets").width(w));

        let query = self.code_snippets.query.trim().to_ascii_lowercase();
        let filtering = !query.is_empty();
        let selected_idx = self
            .code_snippets
            .selected
            .min(CODE_SNIPPETS.len().saturating_sub(1));
        self.code_snippets.selected = selected_idx;

        let mut double_clicked = None;
        let tree_max_h = (ui.available_height() - 190.0).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("code-snippets-tree")
            .max_height(tree_max_h)
            .auto_shrink([false, true])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::S1;
                for (cat_idx, cat_name) in SNIPPET_CATEGORIES.iter().enumerate() {
                    let matching: Vec<usize> = CODE_SNIPPETS
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| {
                            s.category == cat_idx
                                && (!filtering
                                    || s.title.to_ascii_lowercase().contains(&query)
                                    || s.description.to_ascii_lowercase().contains(&query)
                                    || cat_name.to_ascii_lowercase().contains(&query)
                                    || s.as3.to_ascii_lowercase().contains(&query)
                                    || s.as2.to_ascii_lowercase().contains(&query))
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if filtering && matching.is_empty() {
                        continue;
                    }
                    let open = filtering || self.code_snippets.open_categories[cat_idx];
                    let folder_kind = if open {
                        c::TreeItemType::FolderOpen
                    } else {
                        c::TreeItemType::FolderClosed
                    };
                    let folder_resp = c::TreeItem::new(cat_name, folder_kind).show(ui);
                    if !filtering && (folder_resp.toggle || folder_resp.row.clicked()) {
                        self.code_snippets.open_categories[cat_idx] = !open;
                    }
                    if open {
                        for idx in matching {
                            let snippet = &CODE_SNIPPETS[idx];
                            let resp = c::TreeItem::new(
                                snippet.title,
                                c::TreeItemType::Symbol(i16::SNIPPET),
                            )
                            .depth(1)
                            .selected(self.code_snippets.selected == idx)
                            .show(ui);
                            if resp.row.clicked() {
                                self.code_snippets.selected = idx;
                            }
                            if resp.row.double_clicked() {
                                self.code_snippets.selected = idx;
                                double_clicked = Some(idx);
                            }
                        }
                    }
                }
            });

        let snippet = &CODE_SNIPPETS[self.code_snippets.selected];
        let language = self.active_snippet_language();
        let (target, _) = if snippet.uses_target {
            self.snippet_target_instance()
        } else {
            ("orbit_mc".to_owned(), None)
        };
        let preview_body = snippet.render_body(language, &target);

        ui.add_space(space::S1);
        egui::Frame::new()
            .fill(nf.bg_field)
            .stroke(egui::Stroke::new(1.0_f32, nf.border_subtle))
            .corner_radius(egui::CornerRadius::same(radius::MD as u8))
            .inner_margin(egui::Margin::same(space::S3 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                caption(ui, snippet.description);
                ui.add_space(space::S1);
                for line in preview_body.lines() {
                    ui.label(rich(line, TextStyle::Mono).color(nf.text_primary));
                }
            });

        ui.add_space(space::S1);
        let can_insert = self.symbol().kind != SymbolKind::Button;
        let insert_label = format!("Insert at frame {}", self.frame + 1);
        let mut insert_clicked = double_clicked;
        ui.horizontal(|ui| {
            if ui.add(c::Button::ghost("Copy")).clicked() {
                let copy_source = snippet.merge_into("", language, &target);
                ui.ctx().copy_text(copy_source);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(can_insert, c::Button::primary(&insert_label))
                    .clicked()
                {
                    insert_clicked = Some(self.code_snippets.selected);
                }
            });
        });

        if let Some(idx) = insert_clicked {
            self.insert_code_snippet(idx);
        }
    }
}
