#[macro_use] extern crate enum_primitive;

use std::sync::mpsc;

use libremarkable::framebuffer::{core, FramebufferDraw, FramebufferRefresh};
use libremarkable::framebuffer::{FramebufferBase};

use rusttype::Font;

use armrest::{ui, gesture, ml};

use libremarkable::input::ev::EvDevContext;
use libremarkable::input::multitouch::MultitouchEvent;
use libremarkable::input::{InputDevice, InputEvent};
use std::{fs, process, thread};
use armrest::ui::{Widget, Text, Frame, BoundingBox, Render};
use clap::{App, Arg};
use std::path::Path;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

mod buffer;
mod frame;
mod instruction;
mod options;
mod quetzal;
mod traits;
mod zmachine;

use crate::traits::UI;
use crate::options::Options;
use crate::zmachine::Zmachine;
use armrest::gesture::{Gesture, Tool};
use libremarkable::framebuffer::common::{color, waveform_mode, display_temp, dither_mode, DRAWING_QUANT_BIT};
use libremarkable::framebuffer::refresh::PartialRefreshMode;
use armrest::ink::Ink;
use std::time::Instant;
use armrest::ml::{Recognizer, Spline, LanguageModel};
use libremarkable::cgmath::{Point2, Vector2};

enum Action {
    Print(String),
    Save(Vec<u8>),
    Restore,
}


struct RmUI {
    channel: mpsc::Sender<Action>,
}

impl UI for RmUI {
    fn new() -> Box<Self> where
        Self: Sized {
        unimplemented!()
    }

    fn clear(&self) {
    }

    fn print(&mut self, text: &str) {
        eprintln!("print: {}", text);
        self.channel.send(Action::Print(text.to_string()));
    }

    fn debug(&mut self, text: &str) {
        eprintln!("debug: {}", text);
    }

    fn print_object(&mut self, object: &str) {
        eprintln!("print_obj: {}", object);
        self.channel.send(Action::Print(object.to_string()));
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
                Some(Action::Save(base64::decode(&save).unwrap()))
            },
            "restore" =>  Some(Action::Restore),
            _ => None,
        };

        if let Some(action) = maybe_action {
            self.channel.send(action).unwrap();
        }
    }
}

#[derive(Debug)]
struct Dict(Vec<String>);

impl Dict {
    const VALID: f32 = 1.0;
    // Tradeoff: you want this to be small, since any plausible input
    // is likely to do something more useful than nothing at all.
    // However! If a word is not in the dictionary, then choosing a totally
    // implausible word quite far from the input may make the recognizer seem
    // worse than it is.
    // The right value here will depend on both the quality of the model,
    // dictionary size, and some more subjective things.
    const INVALID: f32 = 0.0;
}

impl LanguageModel for &Dict {
    fn odds(&self, input: &str, ch: char) -> f32 {
        let Dict(words) = self;

        // TODO: use the real lexing rules from https://inform-fiction.org/zmachine/standards/z1point1/sect13.html
        if !ch.is_ascii_lowercase() && !(ch == ' ') {
            return Dict::INVALID;
        }

        let word_start =
            input.rfind(' ').map(|i| i+1).unwrap_or(0);

        let prefix = &input[word_start..];

        // The dictionary only has the first six characters of each word!
        if prefix.len() >= 6 {
            return Dict::VALID;
        }

        // If the prefix is in the dictionary, and none of the suffixes of
        // that prefix start with this character, then penalize the char.
        let suffixes: Vec<&str> =
            words.iter()
                .filter(|w| w.starts_with(prefix))
                .map(|w| w.split_at(prefix.len()).1)
                .collect();

        if suffixes.is_empty() {
            Dict::VALID
        } else {
            let has_valid_extension =
                suffixes.iter().any(|s| {
                    if s.is_empty() {
                        ch == ' '
                    } else {
                        s.chars().next() == Some(ch)
                    }
                });

            if has_valid_extension {
                Dict::VALID
            } else {
                Dict::INVALID
            }
        }
    }
}

struct Paged<T> {
    empty: T,
    current_page: usize,
    pages: Vec<T>,
}

impl<T : Widget> Paged<T> {
    pub fn new(widget: T) -> Paged<T> where T:Clone {
        Paged {
            empty: widget.clone(),
            current_page: 0,
            pages: vec![widget],
        }
    }

    pub fn clear(&mut self) where T:Clone {
        self.current_page = 0;
        self.pages = vec![self.empty.clone()];
    }

    pub fn page_relative(&mut self, count: isize, frame: &mut impl Render) {
        if count == 0 {
            return;
        }

        let desired_page =
            (self.current_page as isize + count)
                .max(0)
                .min(self.pages.len() as isize - 1)
                as usize;

        if desired_page == self.current_page {
            eprintln!("Already on the correct page!");
            return;
        }
        self.current_page = desired_page;

        let page = &self.pages[self.current_page];
        frame.clear_bounds(page.bounds());
        page.render(frame);
    }
}

impl<T> Paged<ui::Stack<T>> {
    pub fn push<R : Render>(&mut self, mut widget: T, frame: &mut R)
        where
            T: Widget + Clone,
    {
        // TODO: this function sucks
        let mut page_count = self.pages.len();

        let mut last_page = self.pages.last_mut().expect("Should never be empty!");
        if last_page.remaining().height() < widget.bounds().height() {
            self.pages.push(self.empty.clone());
            page_count += 1;
            last_page = self.pages.last_mut().unwrap();
        }

        if self.current_page + 1 == page_count {
            last_page.push(widget, frame);
        } else {
            last_page.push(widget, &mut ());
        }
    }
}

impl<T: Widget> Widget for Paged<T> {
    type Message = Option<T::Message>;

    fn bounds(&self) -> BoundingBox {
        self.empty.bounds()
    }

    fn update<R: Render>(&mut self, action: &ui::Action, sink: &mut R) -> Option<Self::Message> {
        match action {
            ui::Action::Touch(touch) if touch.length() > 100.0 => {
                eprintln!("{:?} {}", touch, touch.length());
                let delta_x = touch.end.x - touch.start.x;
                if delta_x > 80.0 {
                    eprintln!("Swipe right - go back!");
                    self.page_relative(-1, sink);
                } else if delta_x < -80.0 {
                    eprintln!("Swipe right - go forward!");
                    self.page_relative(1, sink);
                }
                None
            },
            _ => {
                self.pages[self.current_page].update(&action, sink).map(|m| Some(m))
            },
        }
    }

    fn render<R: Render>(&self, sink: &mut R) {
        self.pages[self.current_page].render(sink)
    }

    fn translate(&mut self, vec: Vector2<i32>) {
        self.empty.translate(vec);
        for page in &mut self.pages {
            page.translate(vec);
        }
    }
}

struct Game<'a, 'b> {
    zvm: Zmachine,
    font: Font<'a>,
    actions: mpsc::Receiver<Action>,
    buffer: String,
    body: Paged<ui::Stack<ui::Text<'a>>>,
    // status_left: ui::Text<'a>,
    frame: ui::Frame,
    save_path: Option<&'b Path>,
}

impl Game<'_, '_> {

    pub fn restore(&mut self) {
        let path = self.save_path.expect("Restoring without a save path");
        if let Ok(save_data) = fs::read(path) {
            eprintln!("Restoring from save at {}", path.display());
            let based = base64::encode(&save_data);
            self.zvm.restore(&based);
        }
    }

    pub fn drain_actions(&mut self) {
        while let Ok(action) = self.actions.try_recv() {
            match action {
                Action::Print(result) => {
                    let mut split = result.split('\n');
                    self.buffer.push_str(split.next().unwrap());

                    while let Some(more) = split.next() {
                        for widget in ui::Text::wrap(&self.font, &self.buffer, 1000, 44) {
                            self.body.push(widget, &mut self.frame);
                        }
                        self.buffer.clear();
                        self.buffer.push_str(more);
                    }
                }
                Action::Save(data) => {
                    if let Some(path) = &self.save_path {
                        fs::write(path, data).unwrap();
                    }
                }
                Action::Restore => {
                    self.restore();
                    self.frame.clear();
                    self.body.clear();
                    self.zvm.step();
                }
            }
        }

        // TODO: turn this into a prompt instead!
        if !self.buffer.is_empty() {
            for widget in ui::Text::wrap(&self.font, &self.buffer, 1000, 44) {
                self.body.push(widget, &mut self.frame);
            }
            self.buffer.clear();
        }

        self.frame.refresh();
    }
}

fn main() {

    let matches = App::new("encrusted")
        .version("0.1")
        .about("A zmachine interpreter (on reMarkable!)")
        .arg(
            Arg::with_name("FILE")
                .help("Sets the story file to run")
                .required(true),
        )
        .arg(
            Arg::with_name("SAVE")
                .help("The save file to use")
                .required(false),
        )
        .get_matches();

    let path = Path::new(matches.value_of("FILE").unwrap());

    if !path.is_file() {
        println!(
            "\nCouldn't find game file: \n   {}\n",
            path.to_string_lossy()
        );
        process::exit(1);
    }

    let save_path = matches.value_of("SAVE").map(|s| Path::new(s));

    let mut data = Vec::new();
    let mut file = File::open(path).expect("Error opening file");
    file.read_to_end(&mut data).expect("Error reading file");

    let version = data[0];

    if version == 0 || version > 8 {
        println!(
            "\n\
             \"{}\" has an unsupported game version: {}\n\
             Is this a valid game file?\n",
            path.to_string_lossy(),
            version
        );
        process::exit(1);
    }

    let font_bytes =
        fs::read("/usr/share/fonts/ttf/ebgaramond/EBGaramond-VariableFont_wght.ttf").unwrap();

    let font: Font<'static> = Font::from_bytes(font_bytes).unwrap();

    let mut frame = ui::Frame::from_framebuffer(core::Framebuffer::from_path("/dev/fb0"));

    frame.clear();

    let mut stack = Paged::new(ui::Stack::<Text<'static>>::new(frame.bounds().pad(100)));

    frame.refresh();

    let (vm_tx, vm_rx) = mpsc::channel();

    let mut ui = RmUI {
        channel: vm_tx
    };
    let mut opts = Options::default();

    let mut zvm = Zmachine::new(data, Box::new(ui), opts);

    let dict = Dict(zvm.get_dictionary());

    let mut buffer = String::new();

    let mut game = Game {
        zvm,
        font,
        actions: vm_rx,
        buffer,
        body: stack,
        frame,
        save_path
    };

    if let Some(p) = save_path {
        game.restore()
    }
    game.zvm.step();

    game.drain_actions();

    let (input_tx, input_rx) = mpsc::channel::<InputEvent>();
    // Send all input events to input_rx
    let (ink_tx, ink_rx) = mpsc::channel::<(Ink, Instant, usize)>();
    let (text_tx, text_rx) = mpsc::channel::<(Vec<(String, f32)>, Instant, usize)>();
    EvDevContext::new(InputDevice::Multitouch, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Wacom, input_tx.clone()).start();

    let mut ink_log = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/home/root/encrusted.log");

    if let Err(ioerr) = &ink_log {
        eprintln!("Error when opening ink log file: {}", ioerr);
    }

    let _thread = thread::spawn(move || {
        let mut recognizer: Recognizer<Spline> = ml::Recognizer::new().unwrap();

        for (i, t, n) in ink_rx {
            let string = recognizer
                .recognize(
                    &i,
                    &ml::Beam {
                        size: 4,
                        language_model: &dict,
                    },
                )
                .unwrap();

            if let Ok(log) = &mut ink_log {
                let mut ink_str = String::new();
                i.to_string(&mut ink_str);
                writeln!(log, "{}\t{}", &string[0].0, ink_str).expect("why not?");
                log.flush().expect("why not?");
            }

            text_tx.send((string, t, n)).unwrap();
            // NB: hack! Wake up the event loop when we're ready by sending a fake event.
            input_tx.send(InputEvent::Unknown {}).unwrap();
        }
    });

    let mut gestures = gesture::State::new();

    while let Ok(event) = input_rx.recv() {
        match gestures.on_event(event) {
            Some(Gesture::Ink(Tool::Pen)) => {
                let ink = gestures.current_ink();
                ink_tx
                    .send((ink.clone(), gestures.ink_start(), ink.len()))
                    .unwrap();
            }
            Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                let rect = game.frame.fb.draw_line(from, to, 3, color::BLACK);
                game.frame.fb.partial_refresh(
                    &rect,
                    PartialRefreshMode::Wait,
                    waveform_mode::WAVEFORM_MODE_DU,
                    display_temp::TEMP_USE_REMARKABLE_DRAW,
                    dither_mode::EPDC_FLAG_USE_DITHERING_ALPHA,
                    DRAWING_QUANT_BIT,
                    false,
                );
            }
            Some(Gesture::Tap(touch)) => {
                eprintln!("{:?} {}", touch, touch.length());
                game.body.update(&ui::Action::Touch(touch), &mut game.frame);
                game.frame.refresh();
            }
            _ => {}
        }

        if let Ok((texts, time, len)) = text_rx.try_recv() {
            if gestures.ink_start() == time && gestures.current_ink().len() == len {
                let text = texts[0].0.clone();
                let _ = gestures.take_ink();

                eprintln!("Results: {:?}", texts);

                game.zvm.handle_input(text);
                let done = game.zvm.step();
                if done {
                    eprintln!("Exit requested!");
                    return;
                }
                game.drain_actions();
            }
        }
    }
}