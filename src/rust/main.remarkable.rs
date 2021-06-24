#[macro_use]
extern crate enum_primitive;

use std::{fs, io, mem, process, thread};
use std::collections::BTreeSet;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Sink, Write};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Instant, SystemTime};

use armrest::{gesture, ml, ui};
use armrest::gesture::{Gesture, Tool, Touch};
use armrest::ink::Ink;
use armrest::ml::{LanguageModel, Recognizer, Spline};
use armrest::ui::{Action, BoundingBox, Frame, Paged, Split, Stack, Text, Widget};
use clap::{App, Arg};
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::{core, FramebufferDraw, FramebufferRefresh};
use libremarkable::framebuffer::FramebufferBase;
use libremarkable::framebuffer::common::{color, DISPLAYWIDTH};
use libremarkable::input::{InputDevice, InputEvent};
use libremarkable::input::ev::EvDevContext;
use libremarkable::input::multitouch::MultitouchEvent;
use rusttype::Font;

use crate::options::Options;
use crate::traits::UI;
use crate::zmachine::Zmachine;
use std::io::SeekFrom::Start;

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
const PROMPT_WIDTH: i32 = 22; // For the prompt etc.
const LINE_LENGTH: i32 = 1000;
const TEXT_HEIGHT: i32 = 1408; // 34 * LINE_HEIGHT

enum VmOutput {
    Print(String),
    // PrintObject(String),
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

impl Dict {
    const VALID: f32 = 1.0;
    // Tradeoff: you want this to be small, since any plausible input
    // is likely to do something more useful than nothing at all.
    // However! If a word is not in the dictionary, then choosing a totally
    // implausible word quite far from the input may make the recognizer seem
    // worse than it is.
    // The right value here will depend on both the quality of the model,
    // dictionary size, and some more subjective things.
    const INVALID: f32 = 0.001;

    fn contains_prefix(&self, prefix: &str) -> bool {
        self.0.range(prefix.to_string()..).next().map_or(false, |c| c.starts_with(prefix))
    }
}

impl LanguageModel for &Dict {
    fn odds(&self, input: &str, ch: char) -> f32 {
        let Dict(words) = self;

        // TODO: use the real lexing rules from https://inform-fiction.org/zmachine/standards/z1point1/sect13.html
        if !ch.is_ascii_lowercase() && !(" .,".contains(ch)) {
            return Dict::INVALID;
        }

        let word_start =
            input.rfind(|c| " .,".contains(c)).map(|i| i + 1).unwrap_or(0);

        let prefix = &input[word_start..];

        // The dictionary only has the first six characters of each word!
        if prefix.len() >= 6 {
            return Dict::VALID;
        }

        // If the current character is punctuation, we check that the prefix is a valid word
        if " .,".contains(ch) {
            return if words.contains(prefix) { Dict::VALID } else { Dict::INVALID };
        }

        if self.contains_prefix(prefix) {
            let mut extended = prefix.to_string();
            extended.push(ch);
            if self.contains_prefix(&extended) {
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
    Restore(PathBuf),
}

enum Element {
    Break,
    Line(ui::Text<'static, Msg>),
    Input(bool, ui::Text<'static, Msg>, ui::InputArea<Msg>),
}

impl Widget for Element {
    type Message = Msg;

    fn bounds(&self) -> Vector2<i32> {
        match self {
            Element::Break => Vector2::new(0, LINE_HEIGHT),
            Element::Line(t) => t.bounds() + Vector2::new(PROMPT_WIDTH, 0),
            Element::Input(_, _, i) => i.bounds() + Vector2::new(PROMPT_WIDTH, 0),
        }
    }

    fn render(&self, mut sink: Frame<Msg>) {
        if let Element::Input(true, prompt, _) = self {
            let mut prompt_frame = sink.split_off(Split::Left, PROMPT_WIDTH);
            prompt_frame.split_off(Split::Top, 20);
            prompt.render(prompt_frame);
        } else {
            sink.split_off(Split::Left, PROMPT_WIDTH);
        }

        match self {
            Element::Break => {}
            Element::Line(t) => {
                t.render(sink);
            }
            Element::Input(_, _, input) => {
                input.render(sink);
            }
        }
    }
}

struct Session {
    font: Font<'static>,
    zvm: Zmachine,
    dict: Arc<Dict>,
    actions: mpsc::Receiver<VmOutput>,
    pages: Paged<Stack<Element>>,
    restore: Option<Paged<Stack<GameEntry>>>,
    save_root: PathBuf,
}

impl Session {
    pub fn maybe_new_page(&mut self, padding: i32) {
        let space_remaining = self.pages.last().remaining();
        if padding > space_remaining.y {
            self.pages.push(Stack::new(self.pages.last().bounds()));
        }
    }

    pub fn restore(&mut self, path: &Path) {
        if let Ok(save_data) = fs::read(path) {
            eprintln!("Restoring from save at {}", path.display());
            let based = base64::encode(&save_data);
            self.zvm.restore(&based);
        }
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

    pub fn restore_menu(&mut self, saves: Vec<PathBuf>, bounds: Vector2<i32>) {
        let mut page = Paged::new(Stack::new(bounds));
        for path in saves {
            let text = path.to_string_lossy();
            let widget = Text::layout(&self.font, &text, LINE_HEIGHT);
            page.push_stack(GameEntry {
                path: path.clone(),
                text: widget.on_touch(Some(Msg::Restore(path)))
            })
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
                    self.pages.push_stack(Element::Break);
                }
            }

            for line in paragraph.split("\n") {
                for widget in ui::Text::wrap(&self.font, line, self.pages.bounds().x - 60, LINE_HEIGHT) {
                    self.pages.push_stack(Element::Line(widget));
                }
            }

            first = false;
        }
    }

    pub fn advance(&mut self) -> bool {
        let finished = self.zvm.step();

        if finished {
            unimplemented!("Game stopped!");
        }

        let mut buffer = String::new();
        while let Ok(action) = self.actions.try_recv() {
            match action {
                VmOutput::Print(result) => {
                    buffer.push_str(&result);
                }
                VmOutput::Save(data) => {
                    self.append_buffer(mem::take(&mut buffer));
                    let stuff = ui::Text::layout(&self.font, "SAVE pLACEHOLDER!", 88);
                    self.pages.push_stack(Element::Line(stuff));

                    let now = chrono::offset::Local::now();
                    let save_file_name = format!("{}.sav", now.format("%Y-%m-%d-%H%M%S"));
                    let save_path = self.save_root.join(Path::new(&save_file_name));
                    fs::write(save_path, data).unwrap();
                }
                VmOutput::Restore => {
                    return true;
                }
            }
        }

        self.append_buffer(buffer);

        self.maybe_new_page(88);
        let last_page = self.pages.len() - 1;
        let next_element = self.pages.last().len();

        self.pages.push_stack(Element::Input(
            true,
            ui::Text::layout(&self.font, ">", LINE_HEIGHT).on_touch(None),
            ui::InputArea::new(Vector2::new(600, 88)).on_ink(Some(Msg::Input(last_page, next_element)))
        ));

        false
    }
}

enum GameState {
    // No game loaded... choose from a list.
    Init {
        games: Paged<Stack<GameEntry>>,
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
            actions: vm_rx,
            pages,
            restore: None,
            save_root,
        };

        Ok(session)
    }

    fn init(bounds: BoundingBox, font: Font<'static>, ink_tx: mpsc::Sender<(Ink, Arc<Dict>, usize)>, root_dir: PathBuf) -> Game {
        let mut games = Paged::new(Stack::new(bounds.size()));
        for game_path in Game::list_games(&root_dir).expect(&format!("Unable to list games in {:?}", root_dir)) {
            let path_str = format!("❧ {}", game_path.to_str().unwrap_or("unknown path"));
            let text = Text::layout(&font, &path_str, LINE_HEIGHT);

            games.push_stack(GameEntry {
                path: game_path.clone(),
                text: text.on_touch(Some(Msg::LoadGame(game_path))),
            });
        }

        Game {
            bounds,
            state: GameState::Init { games },
            font,
            ink_tx,
            awaiting_ink: 0,
            root_dir,
        }
    }

    pub fn update(&mut self, action: Action, message: Msg) {
        let text_area_shape = self.bounds();
        match &mut self.state {
            GameState::Init { games } => {
                match message {
                    Msg::Page => {
                        if let Action::Touch(touch) = action {
                            if (touch.start.y - touch.end.y).abs() < 40.0 {
                                let hdiff = touch.end.x - touch.start.x;
                                if hdiff < -80.0 {
                                    games.page_relative(1);
                                } else if hdiff > 80.0 {
                                    games.page_relative(-1);
                                }
                            }
                        }
                    }
                    Msg::LoadGame(game_path) => {
                        let mut session = self.load_game(&game_path).unwrap();
                        let saves = session.load_saves().unwrap();
                        if saves.is_empty() {
                            session.advance();
                        } else {
                            session.restore_menu(saves, self.bounds())
                        }

                        self.state = GameState::Playing { session }
                    }
                    _ => {}
                }
            }
            GameState::Playing { session } => {
                if let Some(restore) = &mut session.restore {
                    match message {
                        Msg::Page => {
                            if let Action::Touch(touch) = action {
                                if (touch.start.y - touch.end.y).abs() < 40.0 {
                                    let hdiff = touch.end.x - touch.start.x;
                                    if hdiff < -80.0 {
                                        restore.page_relative(1);
                                    } else if hdiff > 80.0 {
                                        restore.page_relative(-1);
                                    }
                                }
                            }
                        }
                        Msg::Restore(path) => {
                            session.restore(&path);
                            session.advance();
                            session.restore = None;
                        }
                        _ => {}
                    }
                } else {
                    match message {
                        Msg::Input(page, line) => {
                            if let Action::Ink(ink) = action {
                                let element = &mut session.pages[page][line];
                                match element {
                                    Element::Input(true, _, input) => {
                                        input.ink.append(ink.clone(), 0.5);
                                        self.awaiting_ink += 1;
                                        self.ink_tx
                                            .send((input.ink.clone(), session.dict.clone(), self.awaiting_ink))
                                            .unwrap();
                                    }
                                    _ => {
                                        eprintln!("Strange: got input on an unexpected element. [page={}, line{}]", page, line);
                                    }
                                }
                            }
                        }

                        Msg::Page => {
                            if let Action::Touch(touch) = action {
                                if (touch.start.y - touch.end.y).abs() < 40.0 {
                                    let hdiff = touch.end.x - touch.start.x;
                                    if hdiff < -80.0 {
                                        session.pages.page_relative(1);
                                    } else if hdiff > 80.0 {
                                        session.pages.page_relative(-1);
                                    }
                                }
                            }
                        }
                        Msg::RecognizedText(n, text) => {
                            if n == self.awaiting_ink {
                                if let Some(Element::Input(flag, _, _)) = session.pages.current_mut().last_mut() {
                                    *flag = false;
                                }
                                session.zvm.handle_input(text);
                                if session.advance() {
                                    // Start a new page unless the current page is empty
                                    session.maybe_new_page(TEXT_HEIGHT);
                                    let saves = session.load_saves().unwrap();
                                    session.restore_menu(saves, text_area_shape)
                                };
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

struct GameEntry {
    path: PathBuf,
    text: Text<'static, Msg>,
}

impl Widget for GameEntry {
    type Message = Msg;

    fn bounds(&self) -> Vector2<i32> {
        self.text.bounds()
    }

    fn render(&self, mut sink: Frame<Self::Message>) {
        sink.split_off(Split::Left, PROMPT_WIDTH);
        self.text.render(sink)
    }
}

impl Widget for Game {
    type Message = Msg;

    fn bounds(&self) -> Vector2<i32> {
        self.bounds.bottom_right - Point2::origin()
    }

    fn render(&self, mut frame: Frame<Msg>) {
        frame.split_off(Split::Top, self.bounds.top_left.y);

        let mut body_frame = frame.split_off(Split::Top, TEXT_HEIGHT);
        body_frame.split_off(Split::Left, self.bounds.top_left.x);
        body_frame.on_input(Msg::Page);
        match &self.state {
            GameState::Playing { session } => {
                if let Some(restore) = &session.restore {
                    restore.render(body_frame);
                } else {
                    session.pages.render(body_frame);
                }
            }
            GameState::Init { games } => {
                games.render(body_frame);
            }
        }

        let (page_number, last_page) = match &self.state {
            GameState::Playing { session, .. } => match &session.restore {
                None => (session.pages.current_index(), session.pages.len()),
                Some(saves) => (saves.current_index(), saves.len()),
            }
            GameState::Init { games } => (games.current_index(), games.len()),
        };

        let page_string = (page_number + 1).to_string();
        let page_text = Text::layout(&self.font, &page_string, LINE_HEIGHT).on_touch(None);
        let page_number_start = (DISPLAYWIDTH as i32 - page_text.bounds().x) / 2;

        let mut before = frame.split_off(Split::Left, page_number_start);

        if page_number > 0 {
            let left_arrow = Text::layout(&self.font, "◁", LINE_HEIGHT).on_touch(None);
            left_arrow.render(before.split_off(Split::Right, left_arrow.bounds().x + 16));
            mem::drop(before)
        } else {
            mem::drop(before);
        }

        let mut after = frame.split_off(Split::Right, page_number_start);
        if page_number + 1 < last_page {
            let left_arrow = Text::layout(&self.font, "▷", LINE_HEIGHT).on_touch(None);
            left_arrow.render(after);
        } else {
            mem::drop(after);
        }

        page_text.render(frame);
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
        Point2::new(LEFT_MARGIN + LINE_LENGTH, TOP_MARGIN + TEXT_HEIGHT),
    );

    let mut ink_log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(root_dir.join("handwriting.log"));

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
        match gestures.on_event(event) {
            Some(Gesture::Ink(Tool::Pen)) => {
                let ink = gestures.take_ink();

                screen.damage(ink.bounds());

                let center = ink.centroid().map(|c| c as i32);
                for (b, m) in handlers.query(center) {
                    let translated = ink.clone().translate((Point2::origin() - b.top_left).map(|c| c as f32));
                    eprintln!("Inked! {}", translated.len());
                    game.update(Action::Ink(translated), m.clone());
                }

                handlers = screen.draw(&game);
            }
            Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                screen.stroke(from, to);
            }
            Some(Gesture::Tap(touch)) => {
                let center = touch.midpoint().map(|c| c as i32);
                for (b, m) in handlers.query(center) {
                    let translated = touch.translate((Point2::origin() - b.top_left).map(|c| c as f32));
                    eprintln!("Touched! {:?}", translated);
                    game.update(Action::Touch(translated), m.clone());
                }
                handlers = screen.draw(&game);
            }
            _ => {}
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