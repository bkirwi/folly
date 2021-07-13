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
use armrest::ui::{Action, BoundingBox, Frame, Paged, Side, Stack, Text, Widget};
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

use crate::options::Options;
use crate::traits::UI;
use crate::zmachine::Zmachine;
use std::cell::Cell;
use std::rc::Rc;

mod buffer;
mod frame;
mod instruction;
mod options;
mod quetzal;
mod traits;
mod zmachine;

const LEFT_MARGIN: i32 = 200;
const TOP_MARGIN: i32 = 200;
const LINE_HEIGHT: i32 = 44;
const LINE_LENGTH: i32 = 1000;
const TEXT_AREA_HEIGHT: i32 = 1408; // 34 * LINE_HEIGHT

enum VmOutput {
    Print(String),
    Save(Vec<u8>),
    Restore,
}


struct RmUI {
    channel: mpsc::Sender<VmOutput>,
}

impl UI for RmUI {
    fn new() -> Box<Self> where
        Self: Sized {
        unimplemented!()
    }

    fn clear(&self) {}

    fn print(&mut self, text: &str) {
        eprintln!("print: {}", text);
        self.channel.send(VmOutput::Print(text.to_string()));
    }

    fn debug(&mut self, text: &str) {
        eprintln!("debug: {}", text);
    }

    fn print_object(&mut self, object: &str) {
        eprintln!("print_obj: {}", object);
        self.channel.send(VmOutput::Print(object.to_string()));
    }

    fn set_status_bar(&self, left: &str, right: &str) {
        eprintln!("status: {} {}", left, right);
    }

    fn reset(&self) {
        unimplemented!("Unexpected reset!");
    }

    fn get_user_input(&self) -> String {
        unimplemented!("Unexpected get-user-input!");
    }

    fn flush(&mut self) {
        eprintln!("flush");
    }

    fn message(&self, mtype: &str, msg: &str) {
        eprintln!("message: '{}' '{}'", mtype, msg);
        let maybe_action = match mtype {
            "save" => {
                let (_, save) = serde_json::from_str::<(String, String)>(msg).unwrap();
                Some(VmOutput::Save(base64::decode(&save).unwrap()))
            }
            "restore" => Some(VmOutput::Restore),
            _ => None,
        };

        if let Some(action) = maybe_action {
            self.channel.send(action).unwrap();
        }
    }
}

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
    Input(usize, usize),
    Page,
    RecognizedText(usize, String),
    LoadGame(PathBuf),
    Restore(PathBuf, SaveMeta),
    StartFromBeginning,
}

enum TextAlign {
    Left,
    Center,
}

enum Element {
    Break(i32),
    Line(bool, ui::Text<'static, Msg>),
    Input {
        id: usize,
        active: Rc<Cell<usize>>,
        prompt: ui::Text<'static, Msg>,
        area: ui::InputArea<Msg>
    },
    File {
        big_text: Text<'static, Msg>,
        small_text: Text<'static, Msg>,
    },
}

impl Element {
    fn file_display(font: &Font<'static>, big_text: &str, path_str: &str, msg: Option<Msg>) -> Element {
        Element::File {
            big_text: Text::layout(&font, &big_text, LINE_HEIGHT * 4 / 3).on_touch(msg.clone()),
            small_text: Text::layout(&font, &path_str, LINE_HEIGHT * 2 / 3).on_touch(msg)
        }
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
        };
        Vector2::new(width, height)
    }

    fn render(&self, mut sink: Frame<Msg>) {
        let margin_width = (DISPLAYWIDTH as i32 - LINE_LENGTH) / 2;

        let mut prompt_frame = sink.split_off(Side::Left, margin_width);
        match self {
            Element::Input { id, active, prompt, ..} if *id == active.get() => {
                prompt_frame.split_off(Side::Right, 12);
                prompt_frame.render_placed(prompt, 1.0, 0.5);
            }
            Element::File { .. } => {
                if let Some(mut canvas) = prompt_frame.canvas(388) {
                    let bounds = canvas.bounds();
                    let fb = canvas.framebuffer();
                    fb.fill_rect(
                        Point2::new(bounds.bottom_right.x - 12, bounds.top_left.y + 12),
                        Vector2::new(2, bounds.height() as u32 - 18),
                        color::BLACK,
                    );
                }
            }
            _ => {
                mem::drop(prompt_frame);
            }
        }

        sink.split_off(Side::Right, margin_width);

        match self {
            Element::Break(_) => {}
            Element::Line(center, t) => {
                if *center {
                    sink.render_placed(t, 0.5, 0.0);
                } else {
                    t.render(sink);
                }
            }
            Element::Input { area, .. } => {
                area.render(sink);
            }
            Element::File { big_text, small_text } => {
                sink.render_split(big_text, Side::Top, 0.0);
                small_text.render(sink);
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SaveMeta {
    location: String,
    score_and_turn: String,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum SessionState {
    Running,
    Restoring,
    Quitting,
}

struct Session {
    font: Font<'static>,
    zvm: Zmachine,
    dict: Arc<Dict>,
    next_input: usize,
    active_input: Rc<Cell<usize>>,
    actions: mpsc::Receiver<VmOutput>,
    pages: Paged<Stack<Element>>,
    restore: Option<Paged<Stack<Element>>>,
    save_root: PathBuf,
}

impl Session {
    pub fn maybe_new_page(&mut self, padding: i32) {
        let space_remaining = self.pages.last().remaining();
        if padding > space_remaining.y {
            self.pages.push(Stack::new(self.pages.last().size()));
        }
    }

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

    pub fn restore_menu(&mut self, saves: Vec<PathBuf>, bounds: Vector2<i32>, initial_run: bool) {
        let mut page = Paged::new(Stack::new(bounds));

        for widget in Text::wrap(
            &self.font,
            "Select a saved game to restore from the list below. (Your most recent saved game is listed first.)",
            LINE_LENGTH,
            LINE_HEIGHT,
        ) {
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
                    score_and_turn: "0/0".to_string()
                }
            };
            let slug = format!("{} - {}", &meta.location, &meta.score_and_turn);

            page.push_stack(Element::file_display(&self.font, &slug, &text, Some(Msg::Restore(path, meta))));
        }

        if initial_run {
            page.push_stack(Element::Break(LINE_HEIGHT));
            for widget in Text::wrap(
                &self.font,
                "Or, start from the beginning...",
                LINE_LENGTH,
                LINE_HEIGHT,
            ) {
                page.push_stack(Element::Line(false, widget));
            }
            page.push_stack(Element::Break(LINE_HEIGHT));

            page.push_stack(
                Element::file_display(&self.font, "Start from the beginning", "...", Some(Msg::StartFromBeginning))
            );
        }
        self.restore = Some(page);
    }

    pub fn append_buffer(&mut self, buffer: String) {
        if buffer.is_empty() {
            return;
        }

        let paragraphs: Vec<_> = buffer.trim_end_matches(">").trim_end().split("\n\n").collect();

        let mut first = true;
        for paragraph in paragraphs {
            // Avoid adding padding if we're right after an input element, or at the beginning of a page.
            if !first {
                self.maybe_new_page(LINE_HEIGHT);
                if self.pages.last().len() != 0 {
                    self.pages.push_stack(Element::Break(LINE_HEIGHT));
                }
            }

            // TODO: a less embarassing heuristic for the title paragraph
            if paragraph.contains("Serial number") && (paragraph.contains("Infocom") || paragraph.contains("Inform")) {
                // We think this is a title slug!
                let mut paragraph_iter =paragraph.split("\n");

                if let Some(title) = paragraph_iter.next() {
                    self.maybe_new_page(LINE_HEIGHT * 16);

                    for widget in ui::Text::wrap(&self.font, title, self.pages.size().x, LINE_HEIGHT * 2) {
                        self.pages.push_stack(Element::Line(true, widget));
                    }

                    for line in paragraph_iter {
                        let widget = Text::layout(&self.font, line, LINE_HEIGHT);
                        self.pages.push_stack(Element::Line(true, widget));
                    }
                }
            } else {
                for line in paragraph.split("\n") {
                    for widget in ui::Text::wrap(&self.font, line, self.pages.size().x, LINE_HEIGHT) {
                        self.pages.push_stack(Element::Line(false, widget));
                    }
                }
            }

            first = false;
        }
    }

    pub fn advance(&mut self) -> SessionState {
        let finished = self.zvm.step();

        if finished {
            return SessionState::Quitting;
        }

        let mut buffer = String::new();
        while let Ok(action) = self.actions.try_recv() {
            match action {
                VmOutput::Print(result) => {
                    buffer.push_str(&result);
                }
                VmOutput::Save(data) => {
                    self.append_buffer(mem::take(&mut buffer));

                    self.pages.push_stack(
                        Element::Line(false, Text::layout(&self.font, "Saving game...", LINE_HEIGHT))
                    );

                    let (place, score) = self.zvm.get_status();
                    let now = chrono::offset::Local::now();
                    let save_file_name = format!("{}.sav", now.format("%Y-%m-%dT%H:%M:%S"));
                    let save_path = self.save_root.join(Path::new(&save_file_name));
                    fs::write(&save_path, data).unwrap();

                    let meta = SaveMeta {
                        location: place,
                        score_and_turn: score,
                    };
                    let meta_json = serde_json::to_string(&meta).unwrap();
                    let meta_path = save_path.with_extension("meta");
                    fs::write(&meta_path, meta_json).unwrap();

                    self.pages.push_stack(Element::Break(LINE_HEIGHT/2));
                    self.pages.push_stack(Element::file_display(
                        &self.font,
                        &format!("{} - {}", &meta.location, &meta.score_and_turn),
                        save_path.to_string_lossy().borrow(),
                        None
                    ));
                    self.pages.push_stack(Element::Break(LINE_HEIGHT/2));
                }
                VmOutput::Restore => {
                    return SessionState::Restoring;
                }
            }
        }

        self.append_buffer(buffer);

        self.maybe_new_page(88);
        let last_page = self.pages.len() - 1;
        let next_element = self.pages.last().len();

        let input_id = self.next_input;
        self.next_input += 1;
        self.active_input.set(input_id);
        self.pages.push_stack(Element::Input {
            id: input_id,
            active: self.active_input.clone(),
            prompt: ui::Text::layout(&self.font, "☞", LINE_HEIGHT)
                .on_touch(Some(Msg::RecognizedText(usize::MAX, "".to_string()))),
            area: ui::InputArea::new(Vector2::new(600, 88))
                .on_ink(Some(Msg::Input(last_page, next_element))),
        });

        SessionState::Running
    }
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
    font: Font<'static>,
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
                        if ext == "z3" {
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

        let (vm_tx, vm_rx) = mpsc::channel();

        let mut ui = RmUI {
            channel: vm_tx
        };
        let mut opts = Options::default();

        let mut zvm = Zmachine::new(data, Box::new(ui), opts);

        let dict = Dict(zvm.get_dictionary().drain(..).collect());

        let mut pages = Paged::new(Stack::new(self.bounds.size()));

        // TODO: get the basename and join to the main root
        // Will allow shipping game files at other file paths in the future
        let save_root = path.with_extension("");
        if !save_root.exists() {
            fs::create_dir(&save_root)?;
        }
        let mut session = Session {
            font: self.font.clone(),
            zvm,
            dict: Arc::new(dict),
            next_input: 0,
            active_input: Rc::new(Cell::new(usize::MAX)),
            actions: vm_rx,
            pages,
            restore: None,
            save_root,
        };

        Ok(session)
    }

    fn game_page(font: &Font<'static>, size: Vector2<i32>, root_dir: &Path) -> Paged<Stack<Element>> {
        let mut games = Paged::new(Stack::new(size));

        for widget in Text::wrap(
            font,
            "Welcome to Encrusted! Choose a game from the list to get started.",
            LINE_LENGTH,
            LINE_HEIGHT,
        ) {
            games.push_stack(Element::Line(false, widget));
        }
        games.push_stack(Element::Break(LINE_HEIGHT));

        for game_path in Game::list_games(root_dir).expect(&format!("Unable to list games in {:?}", root_dir)) {
            let path_str = game_path.to_string_lossy().to_string();
            let slug = game_path.file_stem().map_or("unknown".to_string(), |os| os.to_string_lossy().to_string());

            games.push_stack(Element::file_display(font, &slug, &path_str, Some(Msg::LoadGame(game_path))));
        }
        games
    }

    fn init(bounds: BoundingBox, font: Font<'static>, ink_tx: mpsc::Sender<(Ink, Arc<Dict>, usize)>, root_dir: PathBuf) -> Game {
        Game {
            bounds,
            state: GameState::Init { games: Game::game_page(&font, bounds.size(), &root_dir) },
            font,
            ink_tx,
            awaiting_ink: 0,
            root_dir,
        }
    }

    pub fn update(&mut self, action: Action, message: Msg) {
        let text_area_shape = self.size();
        match message {
            Msg::Input(page, line) => {
                if let Action::Ink(ink) = action {
                    if let GameState::Playing { session} = &mut self.state {
                        let element = &mut session.pages[page][line];
                        match element {
                            Element::Input { id, active, area, .. } if *id == active.get() => {
                                area.ink.append(ink.clone(), 0.5);
                                self.awaiting_ink += 1;
                                self.ink_tx
                                    .send((area.ink.clone(), session.dict.clone(), self.awaiting_ink))
                                    .unwrap();
                            }
                            _ => {
                                eprintln!("Strange: got input on an unexpected element. [page={}, line{}]", page, line);
                            }
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
                if n == usize::MAX {
                    // Ignore any pending inks!
                    self.awaiting_ink += 1;
                }
                if n == self.awaiting_ink || n == usize::MAX {
                    session.zvm.handle_input(text);
                    match session.advance() {
                        SessionState::Running => {}
                        SessionState::Restoring => {
                            // Start a new page unless the current page is empty
                            // session.maybe_new_page(TEXT_AREA_HEIGHT);
                            let saves = session.load_saves().unwrap();
                            session.restore_menu(saves, text_area_shape, false)
                        }
                        SessionState::Quitting => {
                            self.state = GameState::Init { games: Game::game_page(&self.font, self.size(), &self.root_dir) };
                        }
                    };
                }
            }
            Msg::LoadGame(game_path) => {
                let mut session = self.load_game(&game_path).unwrap();
                let saves = session.load_saves().unwrap();
                if saves.is_empty() {
                    let state = session.advance();
                    assert_eq!(state, SessionState::Running);
                } else {
                    session.restore_menu(saves, self.size(), true)
                }

                self.state = GameState::Playing { session }
            }
            Msg::Restore(path, meta) => {
                if let GameState::Playing { session }  = &mut self.state {
                    session.pages.push_stack(
                        Element::Line(false, Text::layout(&self.font, "Restoring game...", LINE_HEIGHT))
                    );
                    session.pages.push_stack(Element::Break(LINE_HEIGHT / 2));
                    session.pages.push_stack(Element::file_display(
                        &self.font,
                        &format!("{} - {}", &meta.location, &meta.score_and_turn),
                        &path.to_string_lossy(),
                        None,
                    ));
                    session.pages.push_stack(Element::Break(LINE_HEIGHT / 2));
                    session.restore(&path);
                    let state = session.advance();
                    assert_eq!(state, SessionState::Running);
                    session.restore = None;
                }
            }
            Msg::StartFromBeginning => {
                if let GameState::Playing { session }  = &mut self.state {
                    let state = session.advance();
                    assert_eq!(state, SessionState::Running);
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

    fn render(&self, mut frame: Frame<Msg>) {
        frame.on_input(Msg::Page);
        frame.split_off(Side::Top, self.bounds.top_left.y);

        let mut body_frame = frame.split_off(Side::Top, TEXT_AREA_HEIGHT);

        let current_pages = match &self.state {
            GameState::Playing { session, .. } => match &session.restore {
                None => &session.pages,
                Some(saves) => saves,
            }
            GameState::Init { games } => games,
        };

        current_pages.render(body_frame);

        let page_number = current_pages.current_index();
        let page_string = (current_pages.current_index() + 1).to_string();
        let page_text = Text::layout(&self.font, &page_string, LINE_HEIGHT);

        let page_number_start = (DISPLAYWIDTH as i32 - page_text.size().x) / 2;

        let mut frame = frame.split_off(Side::Top, LINE_HEIGHT * 2);

        let mut before = frame.split_off(Side::Left, page_number_start);

        if page_number > 0 {
            let left_arrow = Text::layout(&self.font, "<", LINE_HEIGHT * 2 / 3);
            before.render_placed(&left_arrow, 0.98, 0.5);
        } else {
            mem::drop(before);
        }

        let mut after = frame.split_off(Side::Right, page_number_start);
        if page_number + 1 < current_pages.len() {
            let left_arrow = Text::layout(&self.font, ">", LINE_HEIGHT * 2 / 3);
            after.render_placed(&left_arrow, 0.02, 0.5)
        } else {
            mem::drop(after);
        }

        frame.render_placed(&page_text, 0.5, 0.5);
    }
}

fn main() {
    let root_dir = PathBuf::from(
        std::env::var("ENCRUSTED_ROOT").unwrap_or("/home/root/encrusted".to_string())
    );

    let font_bytes =
        fs::read("/usr/share/fonts/ttf/ebgaramond/EBGaramond-VariableFont_wght.ttf").unwrap();

    let font: Font<'static> = Font::from_bytes(font_bytes).unwrap();

    let mut screen = ui::Screen::new(core::Framebuffer::from_path("/dev/fb0"));
    screen.clear();

    let (ink_tx, ink_rx) = mpsc::channel::<(Ink, Arc<Dict>, usize)>();
    let (text_tx, text_rx) = mpsc::channel::<(Vec<(String, f32)>, usize)>();

    let page_bounds = BoundingBox::new(
        Point2::new(LEFT_MARGIN, TOP_MARGIN),
        Point2::new(LEFT_MARGIN + LINE_LENGTH, TOP_MARGIN + TEXT_AREA_HEIGHT),
    );

    let mut ink_log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(root_dir.join("ink.log"));

    if let Err(ioerr) = &ink_log {
        eprintln!("Error when opening ink log file: {}", ioerr);
    }

    let mut game = Game::init(
        page_bounds,
        font,
        ink_tx,
        root_dir,
    );

    let mut handlers = screen.draw(&game);

    let (input_tx, input_rx) = mpsc::channel::<InputEvent>();
    EvDevContext::new(InputDevice::Multitouch, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Wacom, input_tx.clone()).start();

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

            text_tx.send((string, n)).unwrap();
            // NB: hack! Wake up the event loop when we're ready by sending a fake event.
            input_tx.send(InputEvent::Unknown {}).unwrap();
        }
    });

    let mut gestures = gesture::State::new();

    while let Ok(event) = input_rx.recv() {
        let action_opt = match gestures.on_event(event) {
            Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                screen.stroke(from, to);
                None
            },
            Some(Gesture::Ink(Tool::Pen)) => {
                let ink = gestures.take_ink();
                screen.damage(ink.bounds());
                Some(Action::Ink(ink))
            },
            Some(Gesture::Tap(touch)) => {
                Some(Action::Touch(touch))
            },
            _ => None,
        };

        if let Some(action) = action_opt {
            let center = action.center().map(|c| c as i32);
            for (b, m) in handlers.query(center) {
                let translated = action.clone().translate((Point2::origin() - b.top_left));
                game.update(translated, m.clone());
            }
            handlers = screen.draw(&game);
        }

        // We don't want to change anything if the user is currently interacting with the screen.
        if gestures.current_ink().len() == 0 {
            if let Ok((texts, n)) = text_rx.try_recv() {
                eprintln!("Results: {:?}", texts);

                let text = texts[0].0.clone();

                game.update(Action::Unknown, Msg::RecognizedText(n, text));

                handlers = screen.draw(&game);
            }
        }
    }
}