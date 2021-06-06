#[macro_use] extern crate enum_primitive;

use std::sync::mpsc;

use libremarkable::framebuffer::{core, FramebufferDraw, FramebufferRefresh};
use libremarkable::framebuffer::{FramebufferBase};

use rusttype::Font;

use armrest::{ui, gesture, ml};

use libremarkable::input::ev::EvDevContext;
use libremarkable::input::multitouch::MultitouchEvent;
use libremarkable::input::{InputDevice, InputEvent};
use std::{fs, process, thread, mem};
use armrest::ui::{Widget, Text, Frame, BoundingBox, Action, Split};
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
use armrest::ink::Ink;
use std::time::Instant;
use armrest::ml::{Recognizer, Spline, LanguageModel};
use libremarkable::cgmath::{Point2, Vector2};
use libremarkable::framebuffer::common::color;
use std::collections::BTreeSet;
use std::ops::Bound;

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

    fn clear(&self) {
    }

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
            },
            "restore" =>  Some(VmOutput::Restore),
            _ => None,
        };

        if let Some(action) = maybe_action {
            self.channel.send(action).unwrap();
        }
    }
}

#[derive(Debug)]
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
            input.rfind(|c| " .,".contains(c)).map(|i| i+1).unwrap_or(0);

        let prefix = &input[word_start..];

        // The dictionary only has the first six characters of each word!
        if prefix.len() >= 6 {
            return Dict::VALID;
        }

        if " .,".contains(ch) {
            return if words.contains(prefix) { Dict::VALID } else { Dict::INVALID };
        }

        if self.contains_prefix(prefix) {
            let mut extended = prefix.to_string();
            extended.push(ch);
            if self.contains_prefix(&extended) {
                Dict::VALID
            }
            else {
                Dict::INVALID
            }
        } else {
            Dict::VALID
        }
    }
}

enum Element {
    Break,
    Line(ui::Text<'static>),
    Input(ui::Text<'static>, ui::InputArea)
}

impl Widget for Element {
    type Message = Ink;

    fn bounds(&self) -> Vector2<i32> {
        match self {
            Element::Break => Vector2::new(0, 44),
            Element::Line(t) => t.bounds(),
            Element::Input(_, _) => Vector2::new(500, 88),
        }
    }

    fn update(&mut self, action: Action) -> Option<Self::Message> {
        match self {
            Element::Break => None,
            Element::Line(t) => None,
            Element::Input(p, i) => {
                i.update(action.translate(-Vector2::new(p.bounds().x, 0)))
            },
        }
    }

    fn render(&self, mut sink: Frame) {
        match self {
            Element::Break => {}
            Element::Line(t) => {
                t.render(sink);
            }
            Element::Input(p, i) => {
                let mut prompt_frame = sink.split(Split::Horizontal(p.bounds().x));
                prompt_frame.split(Split::Vertical(22));
                p.render(prompt_frame);
                i.render(sink);
            }
        }
    }
}

struct Game<'b> {
    zvm: Zmachine,
    font: Font<'static>,
    actions: mpsc::Receiver<VmOutput>,
    buffer: String,
    body: ui::Spaced<ui::Paged<ui::Stack<Element>>>,
    save_path: Option<&'b Path>,
}

impl Game<'_> {

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
                VmOutput::Print(result) => {
                    self.buffer.push_str(&result);
                }
                VmOutput::Save(data) => {
                    if let Some(path) = &self.save_path {
                        fs::write(path, data).unwrap();
                    }
                }
                VmOutput::Restore => {
                    self.restore();
                    // self.body.clear();
                    self.zvm.step();
                }
            }
        }

        let paragraphs: Vec<_> = self.buffer.trim_end_matches(">").trim_end().split("\n\n").collect();

        // self.buffer.rfind("\n")

        let mut first = true;
        for paragraph in paragraphs {
            if !first {
                self.body.value.push(Element::Break);
            }

            for line in paragraph.split("\n") {
                for widget in ui::Text::wrap(&self.font, line, self.body.value.bounds().x, 44) {
                    self.body.value.push(Element::Line(widget));
                }
            }

            first = false;
        }

        let mut widget = ui::Text::layout(&self.font, ">", 44);
        let prompt_len = widget.bounds().x;
        self.body.value.push(Element::Input(widget, ui::InputArea::new(Vector2::new(600 - prompt_len, 88))));


        // if let Some((last_paragraph, others)) = paragraphs.split_last() {
        //     let mut first = true;
        //     for paragraph in others {
        //         if !first {
        //             self.body.value.push(Element::Break);
        //         }
        //
        //         for line in paragraph.split("\n") {
        //             for widget in ui::Text::wrap(&self.font, line, self.body.value.bounds().x, 44) {
        //                 self.body.value.push(Element::Line(widget));
        //             }
        //         }
        //
        //         first = false;
        //     }
        //
        //     let mut widget = ui::Text::layout(&self.font, last_paragraph.trim(), 44);
        //     let prompt_len = widget.bounds().x;
        //     self.body.value.push(Element::Input(widget, ui::InputArea::new(Vector2::new(600 - prompt_len, 88))));
        // }

        self.buffer.clear();
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

    let mut screen = ui::Screen::new(core::Framebuffer::from_path("/dev/fb0"));
    screen.clear();

    let mut stack = ui::Paged::new(ui::Stack::<Element>::new(screen.size() - Vector2::new(400, 200)));

    let (vm_tx, vm_rx) = mpsc::channel();

    let mut ui = RmUI {
        channel: vm_tx
    };
    let mut opts = Options::default();

    let mut zvm = Zmachine::new(data, Box::new(ui), opts);

    let dict = Dict(zvm.get_dictionary().drain(..).collect::<BTreeSet<String>>());

    let mut buffer = String::new();

    let mut game = Game {
        zvm,
        font,
        actions: vm_rx,
        buffer,
        body: ui::Spaced::new(Vector2::new(100, 100), stack),
        save_path
    };

    if let Some(p) = save_path {
        game.restore()
    }
    game.zvm.step();
    game.drain_actions();
    screen.draw(&game.body);

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
                writeln!(log, "{}\t{}", &string[0].0, i).expect("why not?");
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

                game.body.update(ui::Action::Ink(ink.clone()));
                screen.draw(&game.body)
            }
            Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                screen.stroke(from, to);
            }
            Some(Gesture::Tap(touch)) => {
                eprintln!("{:?} {}", touch, touch.length());
                game.body.update(ui::Action::Touch(touch));
                screen.draw(&game.body)
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
                screen.draw(&game.body);
            }
        }
    }
}