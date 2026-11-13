//! The wizard component: its state, the messages that change it, and the work it starts.

use std::sync::{Arc, Mutex, mpsc};

use windows::UI::ViewManagement::UISettings;
use windows_reactor::*;

use nvhvc_core::config::Settings;
use nvhvc_core::hevc::patch::Hdr10Metadata;
use nvhvc_core::pipeline::{self, PipelineOptions, Stage as PipeStage};
use nvhvc_core::probe;
use nvhvc_core::tools::{Tool, Toolset};

use crate::model::*;
use crate::palette::{Accent, Palette};
use crate::{platform, views};

/// Which file a pick, drop or probe is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Source,
    Encode,
}

/// The values Preferences edits in a dialog.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefEditor {
    /// A tool's custom path.
    Tool(Tool),
    Folder,
    Suffix,
}

/// The open Preferences editor dialog and what has been typed into it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PrefEdit {
    pub editor: PrefEditor,
    pub draft: String,
}

/// A button that is recreated when its colours change and needs keyboard focus handed back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusTarget {
    Menu,
    Theme(ThemeMode),
    /// The text box of the Preferences editor dialog, so typing can start at once.
    PrefInput,
}

/// Reported from worker threads through [`Events`].
#[derive(Clone, Debug)]
pub enum Event {
    Progress {
        token: u32,
        stage: usize,
    },
    ColoursChanged,
    /// Esc was pressed in the window.
    Escape,
}

#[derive(Clone)]
pub enum Message {
    ColorScheme(ColorScheme),
    Event(Option<Event>),

    Pick(Slot),
    Dropped(Slot, Option<String>),
    DragHover(bool),
    Probed {
        slot: Slot,
        path: String,
        result: Result<FileProbe, String>,
    },
    PickOutput,

    Continue,
    Back,

    EditRow(usize),
    DraftCode(fn(&mut Draft, u8), Option<u8>),
    DraftRange(Option<bool>),
    DraftNumber(fn(&mut Draft, f64), Option<f64>),
    CancelEdit,
    ApplyEdit,
    ResetRow,
    ResetAll,

    Patch,
    ConfirmReplace,
    CancelReplace,
    RunFinished {
        token: u32,
        result: Result<RunSummary, String>,
    },
    PatchAnother,
    PlayOutput,
    RevealOutput,

    ToggleMenu,
    CloseMenu,
    SetTheme(ThemeMode),
    OpenPrefs,
    OpenHelp,
    OpenAbout,
    CloseAbout,

    ClosePrefs,
    SetContainer(Option<Container>),
    EditPref(PrefEditor),
    PrefDraft(String),
    BrowseFolder,
    SavePref,
    CancelPref,
}

/// Worker threads (pipeline progress, system colour changes) report through a channel. One
/// background task at a time waits on it and hands the next event to `update`, which re-arms
/// the wait: reactor's message senders are UI-thread only, and a background task returns a
/// single message.
struct Events {
    tx: mpsc::Sender<Event>,
    rx: Arc<Mutex<mpsc::Receiver<Event>>>,
}

impl Events {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx: Arc::new(Mutex::new(rx)),
        }
    }

    fn arm(&self, cx: &ComponentContext<Patcher>) {
        let rx = Arc::clone(&self.rx);
        let _ = cx.spawn_background(move |_| Message::Event(rx.lock().unwrap().recv().ok()));
    }
}

pub struct Patcher {
    pub palette: Palette,
    accent: Accent,
    pub theme: ThemeMode,
    events: Events,
    /// Keeps the system colour subscription alive.
    _colours: Option<UISettings>,

    pub stage: Stage,
    pub source: String,
    pub src_probe: Probe,
    pub encode: String,
    pub enc_probe: Probe,
    /// The picked or derived output path; its extension is replaced by `container`'s
    /// (see [`effective_output`]).
    pub output: String,
    /// The output container for this wizard run. Starts at the persisted default; a save
    /// dialog pick with an explicit .mkv/.mp4 changes it.
    pub container: Container,
    /// The output path the user explicitly chose in the save dialog (which already warns about
    /// overwriting). Auto-derived names that already exist get our own confirmation.
    pub output_chosen_for: Option<String>,
    /// Edited HDR10 metadata for the current source; `None` = detected values as-is.
    pub overrides: Option<Hdr10Metadata>,
    pub editing: Option<EditState>,
    /// A file is being dragged over the drop zone on screen.
    pub drag_hover: bool,

    pub run: Run,
    run_token: u32,
    pub confirm_overwrite: bool,

    pub menu_open: bool,
    pub menu_ref: ElementRef<Button>,
    /// The theme switcher's buttons, in `views::theme_index` order.
    pub theme_refs: [ElementRef<Button>; 3],
    /// The button to focus after the next render, with a serial that changes per request. The
    /// menu and theme buttons are recreated when clicked (their colours change, see
    /// `views::restyled`), which would otherwise drop focus mid-keyboard-navigation.
    pub refocus: Option<(u32, FocusTarget)>,
    pub prefs_open: bool,
    pub about_open: bool,
    /// The Preferences editor dialog, while one is open.
    pub pref_edit: Option<PrefEdit>,
    pub pref_input_ref: ElementRef<TextBox>,
}

impl Component for Patcher {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), cx: &ComponentContext<Self>) -> Self {
        let settings = Settings::load();
        let theme = ThemeMode::from_setting(settings.ui_theme.as_deref());
        let accent = platform::accent();
        let events = Events::new();
        let tx = events.tx.clone();
        let colours = platform::watch_colours(move || {
            let _ = tx.send(Event::ColoursChanged);
        });
        let tx = events.tx.clone();
        platform::watch_escape(move || {
            let _ = tx.send(Event::Escape);
        });
        events.arm(cx);
        Self {
            palette: Palette::new(scheme_for(theme), accent),
            accent,
            theme,
            events,
            _colours: colours,
            stage: Stage::Source,
            source: String::new(),
            src_probe: Probe::Idle,
            encode: String::new(),
            enc_probe: Probe::Idle,
            output: String::new(),
            container: Container::from_setting(settings.default_container.as_deref()),
            output_chosen_for: None,
            overrides: None,
            editing: None,
            drag_hover: false,
            run: Run::Idle,
            run_token: 0,
            confirm_overwrite: false,
            menu_open: false,
            menu_ref: ElementRef::new(),
            theme_refs: std::array::from_fn(|_| ElementRef::new()),
            refocus: None,
            prefs_open: false,
            about_open: false,
            pref_edit: None,
            pref_input_ref: ElementRef::new(),
        }
    }

    fn update(&mut self, message: Message, cx: &ComponentContext<Self>) {
        match message {
            Message::ColorScheme(scheme) => self.palette = Palette::new(scheme, self.accent),
            Message::Event(Some(event)) => {
                match event {
                    Event::Progress { token, stage } => {
                        if let Run::Running {
                            token: current,
                            stage: shown,
                        } = &mut self.run
                            && *current == token
                        {
                            *shown = stage;
                        }
                    }
                    Event::ColoursChanged => {
                        self.accent = platform::accent();
                        let scheme = if self.palette.dark {
                            ColorScheme::Dark
                        } else {
                            ColorScheme::Light
                        };
                        self.palette = Palette::new(scheme, self.accent);
                    }
                    // Closes the dialog on top. Preferences itself stays: Esc there also
                    // closes an open container drop-down.
                    Event::Escape => {
                        if self.confirm_overwrite {
                            self.confirm_overwrite = false;
                        } else if self.about_open {
                            self.about_open = false;
                        } else {
                            self.pref_edit = None;
                        }
                    }
                }
                self.events.arm(cx);
            }
            Message::Event(None) => {}

            Message::Pick(slot) => {
                if let Some(path) = platform::pick_file() {
                    self.set_file(slot, path, cx);
                }
            }
            Message::Dropped(slot, path) => {
                self.drag_hover = false;
                if let Some(path) = path {
                    self.set_file(slot, path, cx);
                }
            }
            Message::DragHover(hover) => self.drag_hover = hover,
            Message::Probed { slot, path, result } => {
                let (current, probe) = match slot {
                    Slot::Source => (&self.source, &mut self.src_probe),
                    Slot::Encode => (&self.encode, &mut self.enc_probe),
                };
                // A result for a file that has since been replaced is dropped.
                if *current == path {
                    *probe = match result {
                        Ok(p) => Probe::Ready(p),
                        Err(e) => Probe::Failed(e),
                    };
                }
            }
            Message::PickOutput => {
                let out = effective_output(&self.output, self.container);
                let initial = (!out.is_empty()).then(|| file_name(&out));
                if let Some(path) = platform::pick_save_file(self.container, initial.as_deref()) {
                    let (path, container, rewritten) = normalize_output_pick(&path, self.container);
                    self.output = path.clone();
                    self.container = container;
                    // The save dialog warned about an existing file, but only for the exact
                    // name the user typed; a rewritten extension may collide unseen, so that
                    // case keeps our own overwrite confirmation.
                    self.output_chosen_for = (!rewritten).then_some(path);
                }
            }

            Message::Continue => self.stage = Stage::Encode,
            Message::Back => {
                if matches!(self.run, Run::Done(_)) {
                    // Back from the result screen dismisses it (returns to the encode screen).
                    self.run = Run::Idle;
                } else {
                    self.stage = Stage::Source;
                }
            }

            Message::EditRow(row) => {
                if let Some(effective) = self.effective_metadata() {
                    self.editing = Some(EditState {
                        row,
                        draft: Draft::from_meta(&effective),
                    });
                }
            }
            Message::DraftCode(apply, Some(code)) => self.edit_draft(|d| apply(d, code)),
            Message::DraftRange(Some(full)) => self.edit_draft(|d| d.full_range = full),
            Message::DraftNumber(apply, Some(value)) => self.edit_draft(|d| apply(d, value)),
            Message::DraftCode(_, None)
            | Message::DraftRange(None)
            | Message::DraftNumber(_, None) => {}
            Message::CancelEdit => self.editing = None,
            Message::ApplyEdit => {
                if let (Some(edit), Some(effective)) =
                    (self.editing.take(), self.effective_metadata())
                {
                    self.overrides = Some(edit.draft.apply_row(edit.row, effective));
                }
            }
            Message::ResetRow => {
                // Re-apply this row's detected values over the current override.
                if let (Some(edit), Some(detected), Some(current)) = (
                    self.editing.take(),
                    self.detected_metadata(),
                    self.overrides,
                ) {
                    self.overrides = Some(Draft::from_meta(&detected).apply_row(edit.row, current));
                }
            }
            Message::ResetAll => {
                self.overrides = None;
                self.editing = None;
            }

            Message::Patch => {
                let output = effective_output(&self.output, self.container);
                let user_chosen = self.output_chosen_for.as_deref() == Some(output.as_str());
                if !user_chosen && std::path::Path::new(&output).exists() {
                    self.confirm_overwrite = true;
                } else {
                    self.launch(cx);
                }
            }
            Message::ConfirmReplace => self.launch(cx),
            Message::CancelReplace => self.confirm_overwrite = false,
            Message::RunFinished { token, result } => {
                if token == self.run_token && matches!(self.run, Run::Running { .. }) {
                    self.run = match result {
                        Ok(summary) => Run::Done(summary),
                        Err(e) => Run::Failed(e),
                    };
                }
            }
            Message::PatchAnother => self.reset_wizard(),
            Message::PlayOutput => {
                if let Run::Done(summary) = &self.run {
                    platform::open_path(&summary.output);
                }
            }
            Message::RevealOutput => {
                if let Run::Done(summary) = &self.run {
                    platform::reveal_in_folder(&summary.output);
                }
            }

            Message::ToggleMenu => {
                self.menu_open = !self.menu_open;
                self.focus(FocusTarget::Menu);
            }
            // Dismissed by clicking away: focus returns to the button, as with a flyout.
            Message::CloseMenu => {
                self.menu_open = false;
                self.focus(FocusTarget::Menu);
            }
            Message::SetTheme(mode) => {
                self.focus(FocusTarget::Theme(mode));
                self.theme = mode;
                self.palette = Palette::new(scheme_for(mode), self.accent);
                let mut settings = Settings::load();
                settings.ui_theme = Some(mode.as_setting().to_owned());
                let _ = settings.save();
            }
            Message::OpenPrefs => {
                self.menu_open = false;
                self.prefs_open = true;
            }
            Message::OpenHelp => {
                self.menu_open = false;
                // Same target as the GTK Help entry.
                platform::open_path("https://github.com/Mahir007/nvhvc_hdr10-tool");
            }
            Message::OpenAbout => {
                self.menu_open = false;
                self.about_open = true;
            }
            Message::CloseAbout => self.about_open = false,

            Message::ClosePrefs => {
                self.prefs_open = false;
                self.pref_edit = None;
            }
            Message::SetContainer(Some(container)) => {
                // Persisted immediately, and applied to the wizard too unless the user already
                // picked an output file explicitly (their pick wins). The combo also reports its
                // initial selection, which is the persisted value and changes nothing.
                let mut settings = Settings::load();
                if Container::from_setting(settings.default_container.as_deref()) != container {
                    settings.default_container = Some(container.as_setting().to_owned());
                    let _ = settings.save();
                    if self.output_chosen_for.is_none() {
                        self.container = container;
                    }
                }
            }
            Message::SetContainer(None) => {}
            Message::EditPref(editor) => {
                self.pref_edit = Some(PrefEdit {
                    editor,
                    draft: pref_value(editor),
                });
                self.focus(FocusTarget::PrefInput);
            }
            Message::PrefDraft(text) => {
                if let Some(edit) = &mut self.pref_edit {
                    edit.draft = text;
                }
            }
            Message::BrowseFolder => {
                if let Some(path) = platform::pick_folder()
                    && let Some(edit) = &mut self.pref_edit
                {
                    edit.draft = path;
                }
            }
            Message::SavePref => {
                if let Some(edit) = self.pref_edit.take() {
                    save_pref(edit.editor, edit.draft);
                }
            }
            Message::CancelPref => self.pref_edit = None,
        }
    }

    fn view(&self, _input: &(), cx: &mut ViewContext<Self>) -> View {
        views::root(self, cx)
    }
}

impl Patcher {
    /// The stage on screen: a finished run shows the result screen.
    pub fn shown_stage(&self) -> Stage {
        match (&self.stage, &self.run) {
            (Stage::Encode, Run::Done(_)) => Stage::Result,
            (stage, _) => *stage,
        }
    }

    pub fn processing(&self) -> bool {
        matches!(self.run, Run::Running { .. })
    }

    /// The source's detected HDR10 metadata, once its probe is in.
    pub fn detected_metadata(&self) -> Option<Hdr10Metadata> {
        self.src_probe.ready().map(|p| p.hdr.to_metadata())
    }

    /// The metadata that will be written: the overrides, else the detected values.
    pub fn effective_metadata(&self) -> Option<Hdr10Metadata> {
        self.overrides.or_else(|| self.detected_metadata())
    }

    /// The tools the current wizard inputs need that aren't available (empty until an encode
    /// is picked). Drives the stage-2 warning and the disabled Patch button.
    pub fn missing_tools(&self) -> Vec<Tool> {
        if self.encode.is_empty() {
            return Vec::new();
        }
        let toolset = Toolset::discover(&Settings::load().tool_overrides);
        pipeline::required_tools(std::path::Path::new(&self.encode), self.container)
            .into_iter()
            .filter(|t| !toolset.resolution(*t).is_found())
            .collect()
    }

    fn set_file(&mut self, slot: Slot, path: String, cx: &ComponentContext<Self>) {
        match slot {
            Slot::Source => {
                self.source = path.clone();
                self.src_probe = Probe::Loading;
                // A new source invalidates any edits made against the old one.
                self.overrides = None;
                self.editing = None;
            }
            Slot::Encode => {
                self.encode = path.clone();
                self.enc_probe = Probe::Loading;
                // Settings are re-read so a default folder/suffix change in Preferences applies
                // to the very next pick.
                let settings = Settings::load();
                self.output = default_output(
                    &path,
                    self.container,
                    settings.default_output_dir.as_deref(),
                    settings.default_output_suffix.as_deref(),
                );
            }
        }
        let _ = cx.spawn_background(move |_| {
            let result = probe_file(&path);
            Message::Probed { slot, path, result }
        });
    }

    fn edit_draft(&mut self, f: impl FnOnce(&mut Draft)) {
        if let Some(edit) = &mut self.editing {
            f(&mut edit.draft);
        }
    }

    /// Starts the pipeline on a worker thread; progress arrives as [`Event::Progress`].
    fn launch(&mut self, cx: &ComponentContext<Self>) {
        self.run_token += 1;
        let token = self.run_token;
        self.confirm_overwrite = false;
        self.run = Run::Running { token, stage: 0 };
        let (source, encode) = (self.source.clone(), self.encode.clone());
        let output = effective_output(&self.output, self.container);
        let metadata_override = self.overrides;
        let tx = self.events.tx.clone();
        let _ = cx.spawn_background(move |_| {
            let result = run_pipeline(&source, &encode, &output, metadata_override, &|stage| {
                let _ = tx.send(Event::Progress { token, stage });
            });
            Message::RunFinished { token, result }
        });
    }

    /// Back to an empty wizard; the theme and persisted prefs stay.
    fn reset_wizard(&mut self) {
        self.stage = Stage::Source;
        self.source.clear();
        self.src_probe = Probe::Idle;
        self.encode.clear();
        self.enc_probe = Probe::Idle;
        self.output.clear();
        // A fresh wizard re-reads the persisted default (mirrors GTK's reset).
        self.container = Container::from_setting(Settings::load().default_container.as_deref());
        self.output_chosen_for = None;
        self.overrides = None;
        self.editing = None;
        self.drag_hover = false;
        self.run = Run::Idle;
        self.confirm_overwrite = false;
        self.menu_open = false;
        self.prefs_open = false;
        self.about_open = false;
        self.pref_edit = None;
    }

    fn focus(&mut self, target: FocusTarget) {
        let serial = self.refocus.map_or(0, |(serial, _)| serial.wrapping_add(1));
        self.refocus = Some((serial, target));
    }
}

/// The colour scheme a theme choice renders in.
fn scheme_for(theme: ThemeMode) -> ColorScheme {
    let dark = match theme {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::System => platform::system_is_dark(),
    };
    if dark {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    }
}

/// Runs ffprobe on `path` (called on a worker thread).
fn probe_file(path: &str) -> Result<FileProbe, String> {
    let settings = Settings::load();
    let tools = Toolset::discover(&settings.tool_overrides);
    let ffprobe = tools.require(Tool::Ffprobe).map_err(|e| e.to_string())?;
    let (hdr, info) = probe::probe_media(ffprobe, path.as_ref()).map_err(|e| e.to_string())?;
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    Ok(FileProbe {
        path: path.to_owned(),
        size_bytes,
        info,
        hdr,
    })
}

/// Runs the pipeline (called on a worker thread), reporting each stage's 0..=5 index.
fn run_pipeline(
    source: &str,
    encode: &str,
    output: &str,
    metadata_override: Option<Hdr10Metadata>,
    progress: &dyn Fn(usize),
) -> Result<RunSummary, String> {
    let settings = Settings::load();
    let tools = Toolset::discover(&settings.tool_overrides);
    let opts = PipelineOptions {
        chroma_loc: settings.default_chroma_loc,
        keep_intermediates: false,
        metadata_override,
    };
    let report = pipeline::run_with_progress(
        &tools,
        source.as_ref(),
        encode.as_ref(),
        output.as_ref(),
        &opts,
        &mut |stage| progress(stage_index(stage)),
    )
    .map_err(|e| e.to_string())?;
    // Hold the finished state (checkmark + 100%) briefly so completion registers before the
    // result screen replaces it.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    Ok(RunSummary {
        output: report.output.display().to_string(),
        sps_patched: report.sps_patched,
        sei_insertions: report.sei_insertions,
        frames: report.frames,
        tracks: report.tracks.as_ref().map(nvhvc_core::uimodel::tracks_line),
        dropped_tracks: report.dropped_tracks,
    })
}

/// Maps a pipeline stage to the 0..=5 progress index shown in the processing view.
fn stage_index(stage: PipeStage) -> usize {
    match stage {
        PipeStage::Probing => 0,
        PipeStage::Extracting => 1,
        PipeStage::Patching => 2,
        PipeStage::CapturingTiming => 3,
        PipeStage::Remuxing => 4,
        PipeStage::Done => 5,
    }
}

/// What a Preferences editor starts from: the saved value, or empty when it is unset.
fn pref_value(editor: PrefEditor) -> String {
    let mut settings = Settings::load();
    match editor {
        // The saved override, even when it doesn't resolve, so a wrong path can be corrected.
        // Without one, where the tool was found.
        PrefEditor::Tool(tool) => tool_override(&mut settings, tool)
            .clone()
            .or_else(|| {
                Toolset::discover(&settings.tool_overrides)
                    .resolution(tool)
                    .path()
                    .map(Into::into)
            })
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        PrefEditor::Folder => settings
            .default_output_dir
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(|dir| dir.display().to_string())
            .unwrap_or_default(),
        PrefEditor::Suffix => settings
            .default_output_suffix
            .filter(|suffix| !suffix.trim().is_empty())
            .unwrap_or_default(),
    }
}

/// Persists a Preferences editor's value.
fn save_pref(editor: PrefEditor, value: String) {
    let mut settings = Settings::load();
    match editor {
        PrefEditor::Tool(tool) => {
            let value = value.trim();
            *tool_override(&mut settings, tool) = (!value.is_empty()).then(|| value.into());
        }
        PrefEditor::Folder => {
            let value = value.trim();
            settings.default_output_dir = (!value.is_empty()).then(|| value.into());
        }
        // Kept verbatim (a leading space is meaningful in a suffix); only a blank entry resets
        // to the default.
        PrefEditor::Suffix => {
            settings.default_output_suffix = (!value.trim().is_empty()).then_some(value);
        }
    }
    let _ = settings.save();
}

/// A tool's custom path in settings; `None` means auto-detect.
fn tool_override(settings: &mut Settings, tool: Tool) -> &mut Option<std::path::PathBuf> {
    let o = &mut settings.tool_overrides;
    match tool {
        Tool::Ffmpeg => &mut o.ffmpeg,
        Tool::Ffprobe => &mut o.ffprobe,
        Tool::Mkvmerge => &mut o.mkvmerge,
        Tool::Mkvextract => &mut o.mkvextract,
        Tool::Mp4box => &mut o.mp4box,
    }
}
