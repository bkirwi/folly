#[macro_use]
extern crate enum_primitive;

use std::{fs, io, mem, process, thread};
use std::borrow::Borrow;
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{Read, Sink, Write};
use std::io::SeekFrom::Start;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Instant, SystemTime};

use armrest::{gesture, ml, ui};
use armrest::gesture::{Gesture, Tool, Touch};
use armrest::ink::Ink;
use armrest::ml::{LanguageModel, Recognizer, Spline};
use armrest::ui::{Action, BoundingBox, Frame, Paged, Side, Stack, Text, Widget, TextBuilder, Handlers};
use clap::{App, Arg};
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::{core, FramebufferDraw, FramebufferRefresh, FramebufferIO};
use libremarkable::framebuffer::common::{color, DISPLAYWIDTH, DISPLAYHEIGHT};
use libremarkable::framebuffer::FramebufferBase;
use libremarkable::input::{InputDevice, InputEvent};
use libremarkable::input::ev::EvDevContext;
use libremarkable::input::multitouch::MultitouchEvent;
use rusttype::Font;
use serde::{Deserialize, Serialize};
use itertools::{Itertools, Position};

use crate::options::Options;
use crate::traits::{UI, BaseUI, BaseOutput, Window, TextStyle};
use crate::zmachine::{Zmachine, Step};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

mod buffer;
mod frame;
mod instruction;
mod options;
mod quetzal;
mod traits;
mod zmachine;

const LEFT_MARGIN: i32 = 200;
const TOP_MARGIN: i32 = 150;
const LINE_HEIGHT: i32 = 44;
const MONOSPACE_LINE_HEIGHT: i32 = 34;
const LINE_LENGTH: i32 = 1000;
const TEXT_AREA_HEIGHT: i32 = 1408; // 34 * LINE_HEIGHT
const CHARS_PER_LINE: usize = 64;

const SECTION_BREAK: &str = "* * *";

#[derive(Debug, Clone)]
struct Dict(BTreeSet<String>);

const PUNCTUATION: &str = " .,\"";

impl Dict {
    const VALID: f32 = 1.0;
    // Tradeoff: you want this to be small, since any plausible input
    // is likely to do something more useful than one the game doesn't understand.
    // However! If a word is not in the dictionary, then choosing a totally
    // implausible word quite far from the input may make the recognizer seem
    // worse than it is.
    // The right value here will depend on both the quality of the model,
    // dictionary size, and some more subjective things.
    const INVALID: f32 = 0.001;

    fn contains_prefix(&self, prefix: &String) -> bool {
        self.0.range::<String, _>(prefix..).next().map_or(false, |c| c.starts_with(prefix))
    }
}

impl LanguageModel for &Dict {
    fn odds(&self, input: &str, ch: char) -> f32 {
        let Dict(words) = self;

        // TODO: use the real lexing rules from https://inform-fiction.org/zmachine/standards/z1point1/sect13.html
        if !ch.is_ascii_lowercase() && !(PUNCTUATION.contains(ch)) {
            return Dict::INVALID;
        }

        let word_start =
            input.rfind(|c| PUNCTUATION.contains(c)).map(|i| i + 1).unwrap_or(0);

        let prefix = &input[word_start..];

        // The dictionary only has the first six characters of each word!
        if prefix.len() >= 6 {
            return Dict::VALID;
        }

        // If the current character is punctuation, we check that the prefix is a valid word
        if PUNCTUATION.contains(ch) {
            return if words.contains(prefix) || prefix.is_empty() { Dict::VALID } else { Dict::INVALID };
        }

        // Assume all numbers are valid inputs. (Names that include them normally don't put them in the dictionary.)
        // TODO: think about what happens if dictionary words contain digits.
        if ch.is_ascii_digit() {
            let starts_with_digit = prefix.chars().next().map_or(true, |c| c.is_ascii_digit());
            return if starts_with_digit { Dict::VALID } else { Dict::INVALID };
        }

        let mut prefix_string = prefix.to_string();
        if self.contains_prefix(&prefix_string) {
            prefix_string.push(ch);
            if self.contains_prefix(&prefix_string) {
                Dict::VALID
            } else {
                Dict::INVALID
            }
        } else {
            Dict::VALID
        }
    }

    fn odds_end(&self, prefix: &str) -> f32 {
        self.odds(prefix, ' ')
    }
}

#[derive(Clone, Debug)]
enum Msg {
    Input {
        page: usize,
        line: usize,
        margin: bool,
    },
    Page,
    RecognizedText(usize, String),
    LoadGame(PathBuf),
    Restore(PathBuf, SaveMeta),
    Resume,
}

enum TextAlign {
    Left,
    Center,
}

enum Element {
    Break(i32),
    Line(bool, Text<Msg>),
    Input {
        id: usize,
        active: Rc<Cell<usize>>,
        prompt: Text<Msg>,
        area: ui::InputArea<Msg>,
        message: Msg
    },
    File {
        big_text: Text<Msg>,
        small_text: Text<Msg>,
        message: Option<Msg>,
    },
    UpperWindow(Vec<Text<Msg>>),
}

impl Element {
    fn file_display(font: &Font<'static>, big_text: &str, path_str: &str, msg: Option<Msg>) -> Element {
        Element::File {
            big_text: Text::literal(LINE_HEIGHT * 4 / 3, &font, &big_text),
            small_text: Text::literal(LINE_HEIGHT * 2 / 3, &font, &path_str),
            message: msg
        }
    }

    fn upper_window(font: &Font<'static>, status: Option<(&str, &str)>, lines: &Vec<Vec<(TextStyle, char)>>) -> Option<Element> {

        if lines.is_empty() && status.is_none() {
            return None;
        }

        let mut widgets = vec![];

        if let Some((left, right)) = status {
            // 4 chars minimum padding + 8 for the score/time is reduces the chars available by 12
            let line = format!(" {:width$}  {:8} ", left, right, width=(CHARS_PER_LINE-12));
            widgets.push(Text::literal(MONOSPACE_LINE_HEIGHT, font, &line));
        }

        for line in lines {
            let flattened: String = line.into_iter().take(CHARS_PER_LINE).map(|(_,c)| c).collect();
            let widget = Text::literal(MONOSPACE_LINE_HEIGHT, font, &flattened);
            widgets.push(widget);
        }

        Some(Element::UpperWindow(widgets))
    }
}

impl Widget for Element {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        let width = DISPLAYWIDTH as i32;
        let height = match self {
            Element::Break(n) => *n,
            Element::Line(_, t) => t.size().y,
            Element::Input { area: i, .. } => i.size().y,
            Element::File { .. } => LINE_HEIGHT * 2,
            Element::UpperWindow(lines) => LINE_HEIGHT * (lines.len() + 1) as i32,
        };
        Vector2::new(width, height)
    }

    fn render(&self, handlers: &mut Handlers<Msg>, mut frame: Frame) {
        let margin_width = (DISPLAYWIDTH as i32 - LINE_LENGTH) / 2;

        let mut prompt_frame = frame.split_off(Side::Left, margin_width);
        match self {
            Element::Input { id, active, prompt, message, ..} if *id == active.get() => {
                handlers.push(&prompt_frame, message.clone());
                prompt_frame.split_off(Side::Right, 12);
                prompt.render_placed(handlers, prompt_frame, 1.0, 0.5);
            }
            Element::File { .. } => {
                if let Some(mut canvas) = prompt_frame.canvas(388) {
                    let bounds = canvas.bounds();
                    let fb = canvas.framebuffer();
                    fb.fill_rect(
                        Point2::new(bounds.bottom_right.x - 12, bounds.top_left.y + 12),
                        Vector2::new(2, bounds.size().y as u32 - 18),
                        color::BLACK,
                    );
                }
            }
            _ => {
                mem::drop(prompt_frame);
            }
        }

        frame.split_off(Side::Right, margin_width);

        match self {
            Element::Break(_) => {}
            Element::Line(center, t) => {
                if *center {
                    t.render_placed(handlers, frame, 0.5, 0.0);
                } else {
                    t.render(handlers, frame);
                }
            }
            Element::Input { area, .. } => {
                area.render(handlers, frame);
            }
            Element::File { big_text, small_text , message } => {
                if let Some(m) = message {
                    handlers.push(&frame, m.clone());
                }
                big_text.render_split(handlers, &mut frame, Side::Top, 0.0);
                small_text.render(handlers, frame);
            }
            Element::UpperWindow(lines) => {
                for line in lines {
                    line.render_split(handlers, &mut frame, Side::Top, 0.0);
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SaveMeta {
    location: String,
    score_and_turn: String,
    status_line: Option<String>,
}

struct Session {
    font: Fonts,
    zvm: Zmachine<BaseUI>,
    zvm_state: Step,
    dict: Arc<Dict>,
    next_input: usize,
    active_input: Rc<Cell<usize>>,
    pages: Paged<Stack<Element>>,
    restore: Option<Paged<Stack<Element>>>,
    save_root: PathBuf,
}

impl Session {

    pub fn restore(&mut self, path: &Path) -> io::Result<()> {
        let save_data = fs::read(path)?;
        eprintln!("Restoring from save at {}", path.display());
        let based = base64::encode(&save_data);
        self.zvm.restore(&based);
        Ok(())
    }

    fn load_saves(&self) -> io::Result<Vec<PathBuf>> {
        let mut saves = vec![];
        if self.save_root.exists() {
            for entry in fs::read_dir(&self.save_root)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "sav" {
                            saves.push(path);
                        }
                    }
                }
            }
        }

        saves.sort();
        saves.reverse();

        Ok(saves)
    }

    pub fn maybe_new_page(&mut self, padding: i32) {
        let space_remaining = self.pages.last().remaining();
        if padding > space_remaining.y {
            self.pages.push(Stack::new(self.pages.last().size()));
        }
    }

    fn push_advance_space(&mut self) {
        let height = LINE_HEIGHT.min(self.pages.last().remaining().y);
        let pad_previous_element = match self.pages.last().last() {
            Some(Element::Line(..)) => true,
            Some(Element::File {..}) => true,
            _ => false,
        };
        if height == 0 || !pad_previous_element {
            return;
        }
        self.pages.push_stack(Element::Break(height));
    }

    fn push_element(&mut self, element: Element) {
        self.maybe_new_page(element.size().y);
        if self.pages.last().is_empty() {
            let upper = self.zvm.ui.upper_window();
            if let Some(window) = Element::upper_window(&self.font.monospace, self.zvm.ui.status_line(), upper) {
                self.pages.push_stack(window);
            }
        }
        self.pages.push_stack(element);
    }

    // push a section break, if one is needed (ie. we're not already on a fresh page, and we have the room)
    fn push_section_break(&mut self) {
        let page = self.pages.last();
        if page.is_empty() {
            return;
        }

        let remaining = page.remaining().y;
        if remaining > LINE_HEIGHT * 2 {
            self.push_advance_space();
            self.push_element(
                Element::Line(true, Text::literal(LINE_HEIGHT, &self.font.roman, SECTION_BREAK))
            );
            self.push_advance_space();
        } else {
            self.maybe_new_page(LINE_HEIGHT * 2);
        }
    }

    pub fn restore_menu(&mut self, saves: Vec<PathBuf>, bounds: Vector2<i32>, initial_run: bool) {
        let mut page = Paged::new(Stack::new(bounds));

        let mut builder = TextBuilder::from_font(LINE_HEIGHT, &self.font.roman);
        builder.push_words(
            &self.font.roman,
            LINE_HEIGHT as f32,
            "Select a saved game to restore from the list below, or ",
            None,
        );
        let linked_message = if initial_run {
            "tap here to start from the beginning"
        } else {
            "tap here to return to where you left off"
        };
        builder.push_words(
            &self.font.bold,
            LINE_HEIGHT as f32,
            linked_message,
            Some(Msg::Resume),
        );
        builder.push_words(&self.font.bold, LINE_HEIGHT as f32, ".", None);

        for widget in builder.wrap(LINE_LENGTH, true) {
            page.push_stack(Element::Line(false, widget));
        }
        page.push_stack(Element::Break(LINE_HEIGHT));

        for path in saves {
            let text = path.to_string_lossy().to_string();

            let meta_path = path.with_extension("meta");
            let meta: SaveMeta = if let Ok(data) = fs::read(&meta_path) {
                serde_json::from_slice(&data).expect("Yikes: could not interpret meta file as valid save_meta data")
            } else {
                SaveMeta {
                    location: "Unknown".to_string(),
                    score_and_turn: "0/0".to_string(),
                    status_line: Some("Unknown".to_string()),
                }
            };
            let slug = match &meta.status_line {
                Some(line) => line.clone(),
                None => format!("{} - {}", &meta.location, &meta.score_and_turn),
            };

            page.push_stack(
                Element::file_display(&self.font.monospace, &slug, &text, Some(Msg::Restore(path, meta)))
            );
        }

        self.restore = Some(page);
    }

    pub fn append_buffer(&mut self, buffer: Vec<BaseOutput>) {
        if buffer.is_empty() {
            return;
        }

        let mut lines = vec![];
        let mut current_line = vec![];
        for output in buffer {
            for positioned in output.content.split("\n").with_position() {
                match positioned {
                    Position::Middle(_) | Position::Last(_) => {
                        lines.push(mem::take(&mut current_line));
                    }
                    _ => {}
                }

                current_line.push(
                    BaseOutput { content: positioned.into_inner().to_string(), ..output}
                );
            }
        }

        if let Some(last) = current_line.last_mut() {
            if let Some(stripped) = last.content.strip_suffix(|c| c == ' ' || c == '>') {
                last.content = stripped.to_string();
            }
        }

        lines.push(current_line);

        // TODO: reimplement the title paragraph functionality

        fn blank(line: &[BaseOutput]) -> bool {
            line.iter().all(|o| o.content.trim().is_empty())
        }

        // Drop any trailing blank lines before the input area.
        while lines.last().map_or(false, |l| blank(&l)) {
            lines.pop();
        }

        for line in lines {
            if blank(&line) {
                self.push_advance_space();
                continue;
            }

            let mut text_builder = TextBuilder::from_font(44, &self.font.roman);

            for BaseOutput { style, content } in line {
                // In theory we may want to support multiple of these at once,
                // but we're not required to, so we don't just yet.
                let font = match (style.fixed_pitch(), style.bold(), style.italic()) {
                    (true, _, _) => &self.font.monospace,
                    (_, true, _) => &self.font.bold,
                    (_, _, true) => &self.font.italic,
                    (_, _, _) => &self.font.roman,
                };

                let scale = if style.fixed_pitch() {
                    MONOSPACE_LINE_HEIGHT
                } else {
                    LINE_HEIGHT
                };

                text_builder.push_words(font, scale as f32, &content, None);
            }
            for text in text_builder.wrap(LINE_LENGTH, true) {
                self.push_element(Element::Line(false, text));
            }
        }
    }

    pub fn advance(&mut self) -> Step {
        // if the upper window is displaying some big quote box, collapse it down.
        self.zvm.ui.resolve_upper_height();

        let result = self.zvm.step();
        self.zvm_state = result.clone();

        if self.zvm.ui.is_cleared() {
            self.push_section_break();
            self.maybe_new_page(TEXT_AREA_HEIGHT);
        }

        let buffer = self.zvm.ui.drain_output();
        self.append_buffer(buffer);

        if let Some(upper) = Element::upper_window(&self.font.monospace, self.zvm.ui.status_line(), self.zvm.ui.upper_window()) {
            match self.pages.last_mut().first_mut() {
                Some(e @ Element::UpperWindow(_)) if e.size() == upper.size() => {
                    *e = upper;
                }
                _ => {
                    self.maybe_new_page(TEXT_AREA_HEIGHT);
                    self.pages.push_stack(upper);
                }
            }
        }

        match result {
            Step::Save(data) => {
                self.push_element(
                    Element::Line(false, Text::line(LINE_HEIGHT, &self.font.roman, "Saving game..."))
                );

                let (place, score) = self.zvm.get_status();
                let now = chrono::offset::Local::now();
                let save_file_name = format!("{}.sav", now.format("%Y-%m-%dT%H:%M:%S"));
                let save_path = self.save_root.join(Path::new(&save_file_name));
                fs::write(&save_path, data).unwrap();

                let status_line =
                    if let Some((left, right)) = self.zvm.ui.status_line() {
                        let name_width = self.zvm.options.dimensions.0 - 2;
                        Some(format!("{}  {}", left, right))
                    } else {
                        match self.zvm.ui.upper_window().get(0) {
                            Some(header) => {
                                let string = header.iter().map(|(_, c)| c).collect::<String>();
                                Some(string)
                            },
                            None => None,
                        }
                    };

                let meta = SaveMeta {
                    location: place,
                    score_and_turn: score,
                    status_line,
                };
                let meta_json = serde_json::to_string(&meta).unwrap();
                let meta_path = save_path.with_extension("meta");
                fs::write(&meta_path, meta_json).unwrap();

                self.maybe_new_page(LINE_HEIGHT * 3);
                self.push_element(Element::Break(LINE_HEIGHT/2));
                self.push_element(Element::file_display(
                    &self.font.roman,
                    &meta.status_line.unwrap_or("unknown".to_string()),
                    save_path.to_string_lossy().borrow(),
                    None
                ));
                self.push_element(Element::Break(LINE_HEIGHT/2));

                eprintln!("Game successfully saved!");
                self.zvm.handle_save_result(true);
                self.advance()
            }
            Step::Restore => {
                result
            }
            _ => {
                // Add a prompt
                self.maybe_new_page(88);

                let input_id = self.next_input;
                self.next_input += 1;
                self.active_input.set(input_id);
                self.push_element(Element::Input {
                    id: input_id,
                    active: self.active_input.clone(),
                    prompt: ui::Text::literal(LINE_HEIGHT, &self.font.roman, "☞"),
                    area: ui::InputArea::new(Vector2::new(600, 88)).on_ink(None),
                    message: Msg::Page,
                });

                let last_page = self.pages.len() - 1;
                let next_element = self.pages.last().len() - 1;

                if let Some(Element::Input { message, area, .. }) = self.pages.last_mut().last_mut() {
                    *area = ui::InputArea::new(Vector2::new(600, 88)).on_ink(Some(Msg::Input { page: last_page, line: next_element, margin: false }));
                    *message = Msg::Input { page: last_page, line: next_element, margin: true};
                }

                result
            }
        }
    }
}

#[derive(Clone)]
struct Fonts {
    roman: Font<'static>,
    italic: Font<'static>,
    bold: Font<'static>,
    monospace: Font<'static>,
}

enum GameState {
    // No game loaded... choose from a list.
    Init {
        games: Paged<Stack<Element>>,
    },
    // We're in the middle of a game!
    Playing {
        session: Session,
    },
}

struct Game {
    bounds: BoundingBox,
    state: GameState,
    fonts: Fonts,
    ink_tx: mpsc::Sender<(Ink, Arc<Dict>, usize)>,
    awaiting_ink: usize,
    root_dir: PathBuf,
}

impl Game {
    fn list_games(root_dir: &Path) -> io::Result<Vec<PathBuf>> {
        let mut games = vec![];
        if root_dir.exists() {
            for entry in fs::read_dir(root_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "z3" || ext == "z5" || ext == "z8" {
                            games.push(path);
                        }
                    }
                }
            }
        }
        Ok(games)
    }

    fn load_game(&self, path: &Path) -> io::Result<Session> {
        let mut data = Vec::new();
        let mut file = File::open(path)?;
        file.read_to_end(&mut data)?;

        let version = data[0];

        if version == 0 || version > 8 {
            println!(
                "\n\
                 \"{}\" has an unsupported game version: {}\n\
                 Is this a valid game file?\n",
                path.to_string_lossy(),
                version
            );
            unimplemented!();
        }

        let mut ui = BaseUI::new();

        let mut opts = Options::default();
        opts.dimensions.0 = CHARS_PER_LINE as u16;

        let mut zvm = Zmachine::new(data, ui, opts);

        let dict = Dict(zvm.get_dictionary().into_iter().collect());

        let mut pages = Paged::new(Stack::new(self.bounds.size()));

        // TODO: get the basename and join to the main root
        // Will allow shipping game files at other file paths in the future
        let save_root = path.with_extension("");
        if !save_root.exists() {
            fs::create_dir(&save_root)?;
        }
        let mut session = Session {
            font: self.fonts.clone(),
            zvm,
            zvm_state: Step::Done,
            dict: Arc::new(dict),
            next_input: 0,
            active_input: Rc::new(Cell::new(usize::MAX)),
            pages,
            restore: None,
            save_root,
        };

        Ok(session)
    }

    fn game_page(font: &Font<'static>, size: Vector2<i32>, root_dir: &Path) -> Paged<Stack<Element>> {
        let mut games = Paged::new(Stack::new(size));

        let header = Text::literal(LINE_HEIGHT * 3, font, "ENCRUSTED");
        games.push_stack(Element::Line(true, header));
        games.push_stack(Element::Break(LINE_HEIGHT));

        let game_vec = Game::list_games(root_dir).expect(&format!("Unable to list games in {:?}", root_dir));

        let welcome_message: String = if game_vec.is_empty() {
            format!(
                "Welcome to Encrusted! It looks like you haven't added any games yet... add a Z3 file to {} and restart the app.",
                root_dir.to_string_lossy(),
            )
        } else {
            "Welcome to Encrusted! Choose a game from the list to get started.".to_string()
        };

        for widget in Text::wrap(
            LINE_HEIGHT,
            font,
            &welcome_message,
            LINE_LENGTH,
            true,
        ) {
            games.push_stack(Element::Line(false, widget));
        }
        games.push_stack(Element::Break(LINE_HEIGHT));

        for game_path in game_vec {
            let path_str = game_path.to_string_lossy().to_string();
            let slug = game_path.file_stem().map_or("unknown".to_string(), |os| os.to_string_lossy().to_string());

            games.push_stack(Element::file_display(font, &slug, &path_str, Some(Msg::LoadGame(game_path))));
        }
        games
    }

    fn init(bounds: BoundingBox, fonts: Fonts, ink_tx: mpsc::Sender<(Ink, Arc<Dict>, usize)>, root_dir: PathBuf) -> Game {
        Game {
            bounds,
            state: GameState::Init { games: Game::game_page(&fonts.roman, bounds.size(), &root_dir) },
            fonts: fonts,
            ink_tx,
            awaiting_ink: 0,
            root_dir,
        }
    }

    pub fn update(&mut self, action: Action, message: Msg) {
        let text_area_shape = self.bounds.size();
        match message {
            Msg::Input { page, line, margin } => {
                if let GameState::Playing { session} = &mut self.state {
                    let element = &mut session.pages[page][line];
                    match element {
                        Element::Input { id, active, area, .. } if *id == active.get() => {
                            let submit = match action {
                                Action::Ink(ink) => {
                                    if margin {
                                        false
                                    } else {
                                        area.ink.append(ink, 0.5);
                                        true
                                    }
                                }
                                Action::Touch(_) if margin => area.ink.len() == 0,
                                _ => false
                            };

                            if submit {
                                self.awaiting_ink += 1;
                                self.ink_tx
                                    .send((area.ink.clone(), session.dict.clone(), self.awaiting_ink))
                                    .unwrap();
                            }
                        }
                        _ => {
                            eprintln!("Strange: got input on an unexpected element. [page={}, line{}]", page, line);
                        }
                    }

                }
            }
            Msg::Page => {
                let current_pages = match &mut self.state {
                    GameState::Playing { session, .. } => match &mut session.restore {
                        None => &mut session.pages,
                        Some(saves) => saves,
                    }
                    GameState::Init { games } => games,
                };

                if let Action::Touch(touch) = action {
                    if let Some(side) = touch.to_swipe() {
                        match side {
                            Side::Left => { current_pages.page_relative(1) }
                            Side::Right => { current_pages.page_relative(-1) }
                            _ => {}
                        }
                    }
                }
            }
            Msg::RecognizedText(n, text) => if let GameState::Playing { session }  = &mut self.state {
                if n == self.awaiting_ink {
                    match session.zvm_state.clone() {
                        Step::ReadChar => {
                            let c = text.chars().next().unwrap_or('\n');
                            session.zvm.handle_read_char(c as u8);
                        }
                        Step::ReadLine => {
                            session.zvm.handle_input(text);
                        }
                        other => {
                            unimplemented!("Got input in unexpected state: {:?}", other);
                        }
                    }
                    match session.advance() {
                        Step::Restore => {
                            // Start a new page unless the current page is empty
                            // session.maybe_new_page(TEXT_AREA_HEIGHT);
                            let saves = session.load_saves().unwrap();
                            session.restore_menu(saves, text_area_shape, false);
                        }
                        Step::Done => {
                            self.state = GameState::Init { games: Game::game_page(&self.fonts.roman, self.bounds.size(), &self.root_dir) };
                        }
                        _ => {}
                    };
                }
            }
            Msg::LoadGame(game_path) => {
                let mut session = self.load_game(&game_path).unwrap();
                let saves = session.load_saves().unwrap();
                if saves.is_empty() {
                    let state = session.advance();
                    assert!(state == Step::ReadLine || state == Step::ReadChar);
                } else {
                    session.restore_menu(saves, text_area_shape, true)
                }

                self.state = GameState::Playing { session }
            }
            Msg::Restore(path, meta) => {
                if let GameState::Playing { session }  = &mut self.state {
                    session.push_section_break();
                    session.restore(&path);
                    let state = session.advance();
                    session.restore = None;
                }
            }
            Msg::Resume => {
                if let GameState::Playing { session } = &mut self.state {
                    if session.zvm_state == Step::Restore {
                        session.zvm.handle_restore_result();
                    }
                    let state = session.advance();
                    session.restore = None;
                }
            }
        }
    }
}

impl Widget for Game {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32)
    }

    fn render(&self, handlers: &mut Handlers<Msg>, mut frame: Frame) {
        handlers.push(&frame, Msg::Page);
        frame.split_off(Side::Top, self.bounds.top_left.y);

        let mut body_frame = frame.split_off(Side::Top, TEXT_AREA_HEIGHT);

        let current_pages = match &self.state {
            GameState::Playing { session, .. } => match &session.restore {
                None => &session.pages,
                Some(saves) => saves,
            }
            GameState::Init { games } => games,
        };

        current_pages.render(handlers, body_frame);

        let page_number = current_pages.current_index();
        let page_string = (current_pages.current_index() + 1).to_string();
        let page_text = Text::literal(LINE_HEIGHT,&self.fonts.roman, &page_string);

        let page_number_start = (DISPLAYWIDTH as i32 - page_text.size().x) / 2;

        let mut frame = frame.split_off(Side::Top, LINE_HEIGHT * 2);

        let mut before = frame.split_off(Side::Left, page_number_start);

        if page_number > 0 {
            let left_arrow = Text::line(LINE_HEIGHT * 2 / 3, &self.fonts.roman, "<");
            left_arrow.render_placed(handlers, before, 0.98, 0.5);
        } else {
            mem::drop(before);
        }

        let mut after = frame.split_off(Side::Right, page_number_start);
        if page_number + 1 < current_pages.len() {
            let right_arrow = Text::line(LINE_HEIGHT * 2 / 3, &self.fonts.roman, ">");
            right_arrow.render_placed(handlers, after, 0.02, 0.5)
        } else {
            mem::drop(after);
        }

        page_text.render_placed(handlers, frame, 0.5, 0.5);
    }
}

fn main() {
    let root_dir = PathBuf::from(
        std::env::var("ENCRUSTED_ROOT").unwrap_or("/home/root/encrusted".to_string())
    );

    let font_bytes =
        fs::read("/usr/share/fonts/ttf/ebgaramond/EBGaramond-VariableFont_wght.ttf").unwrap();
    
    let fonts = Fonts {
        roman: Font::from_bytes(include_bytes!("../../rm-fonts/static/EBGaramond-Regular.ttf").as_ref()).unwrap(),
        italic: Font::from_bytes(include_bytes!("../../rm-fonts/static/EBGaramond-Italic.ttf").as_ref()).unwrap(),
        bold: Font::from_bytes(include_bytes!("../../rm-fonts/static/EBGaramond-Bold.ttf").as_ref()).unwrap(),
        monospace: Font::from_bytes(include_bytes!("../../rm-fonts/static/Inconsolata/Inconsolata-Regular.ttf").as_ref()).unwrap(),
    };

    let mut app = armrest::app::App::new();

    let (ink_tx, ink_rx) = mpsc::channel::<(Ink, Arc<Dict>, usize)>();
    let mut text_tx = app.sender();

    let mut ink_log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(root_dir.join("ink.log"));

    if let Err(ioerr) = &ink_log {
        eprintln!("Error when opening ink log file: {}", ioerr);
    }

    let page_bounds = BoundingBox::new(
        Point2::new(LEFT_MARGIN, TOP_MARGIN),
        Point2::new(LEFT_MARGIN + LINE_LENGTH, TOP_MARGIN + TEXT_AREA_HEIGHT),
    );

    let mut game = Game::init(
        page_bounds,
        fonts,
        ink_tx,
        root_dir,
    );

    let _thread = thread::spawn(move || {
        let mut recognizer: Recognizer<Spline> = ml::Recognizer::new().unwrap();

        for (i, dict, n) in ink_rx {
            let string = recognizer
                .recognize(
                    &i,
                    &ml::Beam {
                        size: 4,
                        language_model: dict.as_ref(),
                    },
                )
                .unwrap();

            if let Ok(log) = &mut ink_log {
                writeln!(log, "{}\t{}", &string[0].0, i).expect("why not?");
                log.flush().expect("why not?");
            }

            text_tx.send(Msg::RecognizedText(n, string[0].0.clone()))
        }
    });

    app.run(game, |game, action, msg| {
        game.update(action, msg);
        Ok(())
    })
}