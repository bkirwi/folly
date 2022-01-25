#[macro_use]
extern crate enum_primitive;

#[macro_use]
extern crate lazy_static;

use std::borrow::Borrow;

use std::{fs, io, mem, slice, thread};

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

use armrest::ml;

use armrest::ink::Ink;
use armrest::ml::{Recognizer, Spline};
use armrest::ui::{Canvas, Fragment, Image, Line, Side, Stack, Text, View, Void, Widget};

use libremarkable::cgmath::{Point2, Vector2};

use libremarkable::framebuffer::common::{color, DISPLAYHEIGHT, DISPLAYWIDTH};

use libremarkable::framebuffer::FramebufferDraw;
use libremarkable::image;

use itertools::{Itertools, Position};
use rusttype::Font;
use serde::{Deserialize, Serialize};

use encrusted_heart::options::Options;
use encrusted_heart::traits::{BaseOutput, BaseUI, TextStyle};
use encrusted_heart::zmachine::{Step, Zmachine};
use encrusted_heart::zscii::ZChar;
use regex::Regex;

use armrest::app::{Applet, Component};

use folly::dict::*;
use folly::keyboard::{KeyPress, Keyboard};

/*
For inconsolata, height / width = 0.4766444.
To match the x-height of garamond, we want about 75% the size.
If we choose 64 that gives us a line length of 1006, which is reasonable
 */
const LEFT_MARGIN: i32 = 200;
const TOP_MARGIN: i32 = 150;
const LINE_HEIGHT: i32 = 44;
const MONOSPACE_LINE_HEIGHT: i32 = 33;
const HEADER_SCALE: f32 = 0.9;
const LINE_LENGTH: i32 = 1006;
const TEXT_AREA_HEIGHT: i32 = 1408; // 34 * LINE_HEIGHT
const CHARS_PER_LINE: usize = 64;

const SECTION_BREAK: &str = ">   >   >";

lazy_static! {
    static ref ROMAN: Font<'static> =
        Font::from_bytes(include_bytes!("../fonts/EBGaramond-Regular.ttf").as_ref()).unwrap();
    static ref ITALIC: Font<'static> =
        Font::from_bytes(include_bytes!("../fonts/EBGaramond-Italic.ttf").as_ref()).unwrap();
    static ref BOLD: Font<'static> =
        Font::from_bytes(include_bytes!("../fonts/EBGaramond-Bold.ttf").as_ref()).unwrap();
    static ref MONOSPACE: Font<'static> =
        Font::from_bytes(include_bytes!("../fonts/Inconsolata-Regular.ttf").as_ref()).unwrap();
    static ref MONOSPACE_BOLD: Font<'static> =
        Font::from_bytes(include_bytes!("../fonts/Inconsolata-Bold.ttf").as_ref()).unwrap();
    static ref LONG_WHITESPACE: Regex = Regex::new("\\s\\s\\s+").unwrap();
    static ref PROMPT_ICON: Image = Image::new(
        image::load_from_memory_with_format(include_bytes!("chevron.png"), image::ImageFormat::Png)
            .unwrap()
            .to_rgb()
    );
    static ref KEYBOARD_ICON: Image = Image::new(
        image::load_from_memory_with_format(
            include_bytes!("keyboard.png"),
            image::ImageFormat::Png
        )
        .unwrap()
        .to_rgb()
    );
    static ref GAME_ICON: Image = Image::new(
        image::load_from_memory_with_format(include_bytes!("book.png"), image::ImageFormat::Png)
            .unwrap()
            .to_rgb()
    );
    static ref SAVE_ICON: Image = Image::new(
        image::load_from_memory_with_format(include_bytes!("restore.png"), image::ImageFormat::Png)
            .unwrap()
            .to_rgb()
    );
}

#[derive(Clone, Debug)]
enum Msg {
    Input(Ink),
    Submit,
    ToggleKeyboard,
    PageRelative(isize),
    RecognizedText(usize, String),
    LoadGame(PathBuf),
    Restore(PathBuf, SaveMeta),
    Resume,
    ReadChar(ZChar),
    Shift(usize),
}

enum UserInput {
    Ink(Vec<Ink>),
    String(String),
}

impl UserInput {
    fn is_empty(&self) -> bool {
        match self {
            UserInput::Ink(i) => i.len() == 0,
            UserInput::String(s) => s.is_empty(),
        }
    }
}

enum Element {
    Break(i32),
    Line(bool, Text<Msg>),
    Input {
        active: bool,
        contents: UserInput,
    },
    File {
        icon: &'static Image,
        big_text: Text,
        small_text: Text,
        message: Option<Msg>,
    },
    UpperWindow(Vec<Text<Msg>>),
    CharInput,
}

impl Element {
    fn file_display(
        icon: &'static Image,
        big_text: &str,
        path_str: &str,
        msg: Option<Msg>,
    ) -> Element {
        Element::File {
            icon,
            big_text: Text::literal(LINE_HEIGHT * 4 / 3, &*ROMAN, &big_text),
            small_text: Text::builder(LINE_HEIGHT * 2 / 3, &*ROMAN)
                .font(&*MONOSPACE)
                .scale((MONOSPACE_LINE_HEIGHT * 2 / 3) as f32)
                .literal(&path_str)
                .into_text(),
            message: msg,
        }
    }
}

#[derive(Hash)]
struct Cursor;

impl Fragment for Cursor {
    fn draw(&self, canvas: &mut Canvas) {
        let start = canvas.bounds().top_left + Vector2 { x: 4, y: 2 };
        let end = Point2 {
            y: canvas.bounds().bottom_right.y - 2,
            ..start
        };
        canvas.framebuffer().draw_line(start, end, 2, color::BLACK);
    }
}

impl Widget for Element {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        let width = DISPLAYWIDTH as i32;
        let height = match self {
            Element::Break(n) => *n,
            Element::Line(_, t) => t.size().y,
            Element::Input { .. } => LINE_HEIGHT,
            Element::File { .. } => LINE_HEIGHT * 2,
            Element::UpperWindow(lines) => LINE_HEIGHT * lines.len() as i32,
            Element::CharInput => LINE_HEIGHT,
        };
        Vector2::new(width, height)
    }

    fn render(&self, mut view: View<Msg>) {
        let mut prompt = view.split_off(Side::Left, LEFT_MARGIN);
        const BUTTON_SIZE: i32 = 60;

        fn push_icon(view: &mut View<Msg>, icon: &Image, message: Option<Msg>) {
            let mut button_frame = view.split_off(Side::Right, BUTTON_SIZE);
            if let Some(m) = message {
                button_frame
                    .handlers()
                    .min_size(Vector2::new(100, 100))
                    .on_tap(m);
            }
            icon.void().render_placed(button_frame, 0.5, 1.0);
        }

        match self {
            Element::Input { active, contents } => {
                prompt.split_off(Side::Bottom, 8);
                push_icon(&mut prompt, &*PROMPT_ICON, None);
                if *active {
                    let msg = if contents.is_empty() {
                        Some(Msg::ToggleKeyboard)
                    } else {
                        None
                    };
                    push_icon(&mut prompt, &*KEYBOARD_ICON, msg);
                }
            }
            Element::File { icon, message, .. } => {
                prompt.split_off(Side::Bottom, 42);
                push_icon(&mut prompt, icon, message.clone());
            }
            Element::CharInput => {
                prompt.split_off(Side::Bottom, 8);
                push_icon(&mut prompt, &*KEYBOARD_ICON, Some(Msg::ToggleKeyboard));
            }
            _ => {}
        }
        mem::drop(prompt);

        view.split_off(Side::Right, DISPLAYWIDTH as i32 - LEFT_MARGIN - LINE_LENGTH);

        match self {
            Element::Break(_) => {}
            Element::Line(center, t) => {
                if *center {
                    t.render_placed(view, 0.5, 0.0);
                } else {
                    t.render(view);
                }
            }
            Element::Input {
                active, contents, ..
            } => match contents {
                UserInput::Ink(ink) => {
                    for i in ink {
                        view.annotate(i);
                    }
                    if *active {
                        view.handlers()
                            .min_size(Vector2::new(LINE_LENGTH + LINE_HEIGHT, LINE_HEIGHT * 2))
                            .on_ink(|ink| Msg::Input(ink));
                        view.draw(&Line { y: LINE_HEIGHT - 8 });
                    } else {
                        mem::drop(view);
                    }
                }
                UserInput::String(s) => {
                    let text = Text::builder(LINE_HEIGHT, &*ROMAN)
                        .font(&*MONOSPACE_BOLD)
                        .scale(MONOSPACE_LINE_HEIGHT as f32)
                        .literal(s)
                        .into_text();
                    text.render_split(&mut view, Side::Left, 0.0);
                    if *active {
                        view.draw(&Cursor);
                    } else {
                        mem::drop(view);
                    }
                }
            },
            Element::File {
                big_text,
                small_text,
                message,
                ..
            } => {
                if let Some(m) = message {
                    view.handlers().on_tap(m.clone());
                }
                big_text.void().render_split(&mut view, Side::Top, 0.0);
                small_text.void().render(view);
            }
            Element::UpperWindow(lines) => {
                // TODO: only register these when we're in read-char mode!
                view.handlers()
                    .on_swipe(Side::Top, Msg::ReadChar(ZChar::UP));
                view.handlers()
                    .on_swipe(Side::Bottom, Msg::ReadChar(ZChar::DOWN));
                view.handlers().on_tap(Msg::ReadChar(ZChar::RETURN));
                for line in lines {
                    line.render(view.split_off(Side::Top, LINE_HEIGHT));
                }
            }
            Element::CharInput => {
                view.handlers()
                    .min_size(Vector2::new(0, LINE_HEIGHT * 2))
                    .on_tap(Msg::ReadChar(ZChar(' ' as u8)));

                Text::builder(LINE_HEIGHT, &*BOLD)
                    .literal("[ space ]")
                    .into_text()
                    .render_placed(view, 0.5, 0.0);
            }
        }
    }
}

#[derive(Clone)]
struct Header {
    lines: Vec<Text>,
}

impl Widget for Header {
    type Message = Void;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(LINE_LENGTH, 5 * LINE_HEIGHT)
    }

    fn render(&self, mut view: View<Void>) {
        view.split_off(Side::Bottom, LINE_HEIGHT);
        for line in self.lines.iter().rev() {
            line.render_split(&mut view, Side::Bottom, 0.5)
        }
    }
}

struct Page {
    header: Header,
    body: Stack<Element>,
}

struct Pages {
    contents: Vec<Page>,
    page_number: usize,
    keyboard: Keyboard,
    show_keyboard: bool,
}

impl Pages {
    pub fn new(special: &[char]) -> Pages {
        Pages {
            contents: vec![Page {
                header: Header { lines: vec![] },
                body: Stack::new(Vector2::new(DISPLAYWIDTH as i32, TEXT_AREA_HEIGHT)),
            }],
            page_number: 0,
            keyboard: Keyboard::new(&*MONOSPACE, special),
            show_keyboard: false,
        }
    }

    pub fn current_mut(&mut self) -> &mut Page {
        &mut self.contents[self.page_number]
    }

    pub fn last(&self) -> &Page {
        self.contents.last().expect("Never empty!")
    }

    pub fn last_mut(&mut self) -> &mut Page {
        self.contents.last_mut().expect("Never empty!")
    }

    pub fn page_relative(&mut self, count: isize) {
        if count == 0 {
            return;
        }

        let desired_page = (self.page_number as isize + count)
            .max(0)
            .min(self.contents.len() as isize - 1);

        self.page_number = desired_page as usize;
    }

    fn remaining(&self) -> Vector2<i32> {
        self.last().body.remaining()
    }

    pub fn maybe_new_page(&mut self, padding: i32) {
        let space_remaining = self.remaining();
        if padding > space_remaining.y {
            let Page { header, body } = self.contents.last().unwrap();
            self.contents.push(Page {
                header: header.clone(),
                body: Stack::new(body.size()),
            });
        }
    }

    pub fn replace_header(&mut self, header: Header) {
        let current = &mut self.contents.last_mut().unwrap().header;
        *current = header;
    }

    fn push_advance_space(&mut self) {
        let height = LINE_HEIGHT.min(self.remaining().y);
        let pad_previous_element = match self.contents.last().unwrap().body.last() {
            Some(Element::Line(..)) => true,
            Some(Element::File { .. }) => true,
            Some(Element::Input { .. }) => true,
            Some(Element::UpperWindow(_)) => true,
            _ => false,
        };
        if height == 0 || !pad_previous_element {
            return;
        }
        self.contents
            .last_mut()
            .unwrap()
            .body
            .push(Element::Break(height));
    }

    fn push_element(&mut self, element: Element) {
        self.maybe_new_page(element.size().y);
        self.contents.last_mut().unwrap().body.push(element);
    }

    // push a section break, if one is needed (ie. we're not already on a fresh page, and we have the room)
    fn push_section_break(&mut self) {
        let page = &self.contents.last().unwrap().body;
        if page.is_empty() {
            return;
        }

        let remaining = page.remaining().y;
        if remaining > LINE_HEIGHT * 2 {
            self.push_advance_space();
            self.push_element(Element::Line(
                true,
                Text::literal(LINE_HEIGHT, &*ROMAN, SECTION_BREAK),
            ));
            self.push_advance_space();
        } else {
            self.maybe_new_page(LINE_HEIGHT * 2);
        }
    }
}

impl Widget for Pages {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32)
    }

    fn render(&self, mut view: View<Msg>) {
        view.handlers().on_swipe(Side::Left, Msg::PageRelative(1));
        view.handlers().on_swipe(Side::Right, Msg::PageRelative(-1));

        let Page { header, body } = &self.contents[self.page_number];

        header.void().render_split(&mut view, Side::Top, 0.5);

        if self.show_keyboard && self.page_number + 1 == self.contents.len() {
            let keyboard = &self.keyboard;
            keyboard
                .map(|keypress| match keypress {
                    KeyPress::ZChar(zch) => Msg::ReadChar(zch),
                    KeyPress::Shift(i) => Msg::Shift(i),
                })
                .render_split(&mut view, Side::Bottom, 0.5);
        } else {
            let footer_height = DISPLAYHEIGHT as i32 - 5 * LINE_HEIGHT - TEXT_AREA_HEIGHT;
            let mut footer = view.split_off(Side::Bottom, footer_height);
            footer.split_off(Side::Bottom, 100);

            let roman = &*ROMAN;
            let page_text = Text::literal(LINE_HEIGHT, roman, &(self.page_number + 1).to_string());

            let page_number_start = (DISPLAYWIDTH as i32 - page_text.size().x) / 2;

            let before = footer.split_off(Side::Left, page_number_start);
            if self.page_number > 0 {
                let left_arrow = Text::line(LINE_HEIGHT * 2 / 3, roman, "<");
                left_arrow.render_placed(before, 0.98, 0.5);
            } else {
                mem::drop(before);
            }

            let after = footer.split_off(Side::Right, page_number_start);
            if self.page_number + 1 < self.contents.len() {
                let right_arrow = Text::line(LINE_HEIGHT * 2 / 3, roman, ">");
                right_arrow.render_placed(after, 0.02, 0.5)
            } else {
                mem::drop(after);
            }

            page_text.render_placed(footer, 0.5, 0.5);
        }

        body.render(view);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SaveMeta {
    location: Option<String>,
    score_and_turn: Option<String>,
    status_line: Option<String>,
}

impl SaveMeta {
    fn slug(&self) -> String {
        match &self {
            SaveMeta {
                status_line: Some(line),
                ..
            } => LONG_WHITESPACE.replace_all(line.trim(), " - ").to_string(),
            SaveMeta {
                location: Some(location),
                score_and_turn: Some(score),
                ..
            } => format!("{} - {}", location, score),
            _ => "Unknown".to_string(),
        }
    }
}

struct Session {
    zvm: Zmachine<BaseUI>,
    zvm_state: Step,
    dict: Arc<Dict>,
    pages: Pages,
    restore: Option<Pages>,
    save_root: PathBuf,
}

fn blank(line: &[BaseOutput]) -> bool {
    line.iter().all(|o| o.content.trim().is_empty())
}

impl Session {
    pub fn restore(&mut self, path: &Path) -> io::Result<()> {
        let save_data = fs::read(path)?;
        eprintln!("Restoring from save at {}", path.display());
        self.zvm.restore(&save_data);
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

    pub fn restore_menu(&mut self, saves: Vec<PathBuf>, initial_run: bool) {
        let mut page = Pages::new(&[]);

        let continue_message = if initial_run {
            " to start from the beginning."
        } else {
            " to return to where you left off."
        };

        let lines = Text::builder(LINE_HEIGHT, &*ROMAN)
            .words("Select a saved game to restore from the list below, or ")
            .message(Msg::Resume)
            .words("tap here")
            .no_message()
            .words(continue_message)
            .wrap(LINE_LENGTH, true);

        for widget in lines {
            page.push_element(Element::Line(false, widget));
        }
        page.push_advance_space();

        for path in saves {
            let text = path.to_string_lossy().to_string();

            let meta_path = path.with_extension("meta");
            let meta: SaveMeta = if let Ok(data) = fs::read(&meta_path) {
                serde_json::from_slice(&data)
                    .expect("Yikes: could not interpret meta file as valid save_meta data")
            } else {
                SaveMeta {
                    location: None,
                    score_and_turn: None,
                    status_line: None,
                }
            };

            page.push_element(Element::file_display(
                &*SAVE_ICON,
                &meta.slug(),
                &text,
                Some(Msg::Restore(path, meta)),
            ));
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

                current_line.push(BaseOutput {
                    content: positioned.into_inner().to_string(),
                    ..output
                });
            }
        }

        lines.push(current_line);

        // Strip trailing whitespace and >s
        let trim_chars: &[char] = &[' ', '>'];
        'done: while let Some(last_line) = lines.last_mut() {
            while let Some(last) = last_line.last_mut() {
                let trimmed = last.content.trim_end_matches(trim_chars);
                if trimmed.is_empty() {
                    last_line.pop();
                } else {
                    last.content = trimmed.to_string();
                    break 'done;
                }
            }
            lines.pop();
        }

        // TODO: reimplement the title paragraph functionality

        for line in lines {
            if blank(&line) {
                self.pages.push_advance_space();
                continue;
            }

            let mut text_builder = Text::builder(44, &*ROMAN);

            let justify = line.iter().any(|out| !out.style.fixed_pitch());

            for BaseOutput { style, content } in line {
                // In theory we may want to support multiple of these at once,
                // but we're not required to, so we don't just yet.
                let font = match (style.fixed_pitch(), style.bold(), style.italic()) {
                    (true, _, _) => &*MONOSPACE,
                    (_, true, _) => &*BOLD,
                    (_, _, true) => &*ITALIC,
                    (_, _, _) => &*ROMAN,
                };

                let scale = if style.fixed_pitch() {
                    MONOSPACE_LINE_HEIGHT
                } else {
                    LINE_HEIGHT
                };

                text_builder = text_builder.font(font).scale(scale as f32);
                for (i, word) in content.split(' ').enumerate() {
                    if i != 0 {
                        text_builder = text_builder.space();
                    }
                    if !word.is_empty() {
                        text_builder = text_builder.literal(word);
                    }
                }
            }
            for text in text_builder.wrap(LINE_LENGTH, justify) {
                self.pages.push_element(Element::Line(false, text));
            }
        }
    }

    pub fn advance(&mut self) -> Step {
        // Remove any trailing cruft that shouldn't stay on the page, like menus.
        while matches!(
            self.pages.last().body.last(),
            Some(Element::Break(_)) | Some(Element::UpperWindow(_)) | Some(Element::CharInput)
        ) {
            self.pages.last_mut().body.pop();
        }

        // If there is a trailing input element, mark as inactive
        if let Some(Element::Input { active, .. }) = &mut self.pages.last_mut().body.last_mut() {
            *active = false;
        }

        let on_last_page = self.pages.page_number + 1 == self.pages.contents.len();

        let mut force_keyboard = false;

        // if the upper window is displaying some big quote box, collapse it down.
        self.zvm.ui.resolve_upper_height();

        let result = self.zvm.step();
        self.zvm_state = result.clone();

        if self.zvm.ui.is_cleared() {
            self.pages.push_section_break();
            self.pages.maybe_new_page(TEXT_AREA_HEIGHT);
            if on_last_page {
                self.pages.page_relative(1);
            }
        } else {
            self.pages.maybe_new_page(LINE_HEIGHT);
        }

        // So, there are three things the upper window is typically used for:
        // - The status line. This is recognizable since it uses reverse video on the whole row.
        //   It's usually one line, but eg. Curses has a two-line status.
        //   We wish to render this as the running head of the page, book-style.
        // - A quote box. This is typically a reverse-video box on an empty field.
        //   It's often combined with a status line.
        //   We're happy rendering this as an ordinary block quotation in text.
        // - A menu. These are used for help our about pages, and sometimes story elements.
        //   These can have ~arbitrary content, which means they can't be reliably distinguished
        //   from quote boxes & status lines. Wise to choose a representation for quote boxes and
        //   status lines that doesn't look awful for menus, then.
        let status = if let Some((left, right)) = self.zvm.ui.status_line() {
            // 4 chars minimum padding + 8 for the score/time is reduces the chars available by 12
            let text = format!(
                " {:width$}  {:8} ",
                left,
                right,
                width = (CHARS_PER_LINE - 12)
            );
            vec![Text::literal(30, &*MONOSPACE, &text)]
        } else {
            let upper_window = self.zvm.ui.upper_window();

            // The index of the first non-reverse-video line (ie. no longer the status)
            let cut_index = upper_window
                .iter()
                .position(|p| p.first().map_or(true, |(s, _)| !s.reverse_video()))
                .unwrap_or(upper_window.len());

            fn body_text(line: &[(TextStyle, char)]) -> Text<Msg> {
                let text: String = line.iter().map(|(_, c)| c).collect();
                // We create the builder with ROMAN font so we have the same baseline.
                Text::builder(LINE_HEIGHT, &*ROMAN)
                    .font(&*MONOSPACE)
                    .scale(MONOSPACE_LINE_HEIGHT as f32)
                    .literal(&text)
                    .into_text()
            }

            fn blank_line(line: &[(TextStyle, char)]) -> bool {
                line.iter()
                    .all(|(s, c)| !s.reverse_video() & c.is_ascii_whitespace())
            }

            fn reversed_span(line: &[(TextStyle, char)]) -> Option<(usize, usize)> {
                let mut start = None;
                let mut end = None;
                for (i, (style, char)) in line.iter().enumerate() {
                    if !style.reverse_video() && *char != ' ' {
                        return None;
                    }

                    if start.is_none() {
                        if style.reverse_video() {
                            start = Some(i);
                        }
                    } else if end.is_none() {
                        if !style.reverse_video() {
                            end = Some(i);
                        }
                    } else {
                        if style.reverse_video() {
                            return None;
                        }
                    }
                }

                // FIXME: really ought to pad all lines out to the screen width at this point
                if start.is_some() && end.is_none() {
                    end = Some(line.len());
                }

                Some((start?, end?))
            }

            fn quote_box(upper_window: &[Vec<(TextStyle, char)>]) -> Option<Vec<Text<Msg>>> {
                let top = upper_window
                    .iter()
                    .position(|l| !blank_line(l))
                    .unwrap_or(upper_window.len());

                let bottom = upper_window
                    .iter()
                    .rposition(|l| !blank_line(l))
                    .map_or(0, |i| i + 1);

                if !(top < bottom) {
                    return None;
                }

                let (left, right) = reversed_span(&upper_window[top])?;
                for line in &upper_window[top..bottom] {
                    let (l, r) = reversed_span(line)?;
                    if l != left || r != right {
                        return None;
                    }
                }

                let quote = upper_window[top..bottom]
                    .iter()
                    .map(|line| body_text(&line[left..right]))
                    .collect();

                Some(quote)
            }

            if let Some(quote_box) = quote_box(&upper_window[cut_index..]) {
                for line in quote_box {
                    self.pages.push_element(Element::Line(true, line));
                }
            } else {
                if !upper_window[cut_index..].iter().all(|l| blank_line(l)) {
                    force_keyboard = true;
                    self.pages.push_element(Element::UpperWindow(
                        upper_window[cut_index..]
                            .iter()
                            .map(|line| body_text(&line[..CHARS_PER_LINE.min(line.len())]))
                            .collect(),
                    ));
                }
            }

            upper_window[..cut_index]
                .iter()
                .map(|line| {
                    let text: String = line.iter().take(CHARS_PER_LINE).map(|(_, c)| c).collect();
                    Text::literal(30, &*MONOSPACE, &text)
                })
                .collect::<Vec<_>>()
        };

        self.pages.replace_header(Header { lines: status });

        let was_cleared = self.zvm.ui.is_cleared();
        let buffer = self.zvm.ui.drain_output();
        if was_cleared
            && buffer
                .iter()
                .all(|bo| blank(slice::from_ref(bo)) || bo.style.fixed_pitch())
            && buffer.iter().any(|bo| bo.content.contains(">"))
        {
            // Looks line a menu...
            // TODO: actual menu structure detection!
            force_keyboard = true;
        }
        self.pages.push_advance_space();
        self.append_buffer(buffer);

        match result {
            Step::Save(data) => {
                let now = chrono::offset::Local::now();
                let save_file_name = format!("{}.sav", now.format("%Y-%m-%dT%H:%M:%S"));
                let save_path = self.save_root.join(Path::new(&save_file_name));
                fs::write(&save_path, data).unwrap();

                let status_line = if let Some((left, right)) = self.zvm.ui.status_line() {
                    Some(format!("{}  {}", left, right))
                } else {
                    match self.zvm.ui.upper_window().get(0) {
                        Some(header) => {
                            let string = header.iter().map(|(_, c)| c).collect::<String>();
                            Some(string)
                        }
                        None => None,
                    }
                };

                let meta = SaveMeta {
                    location: None,
                    score_and_turn: None,
                    status_line,
                };
                let meta_json = serde_json::to_string(&meta).unwrap();
                let meta_path = save_path.with_extension("meta");
                fs::write(&meta_path, meta_json).unwrap();

                self.pages.push_advance_space();
                self.pages.push_element(Element::file_display(
                    &*SAVE_ICON,
                    &meta.slug(),
                    save_path.to_string_lossy().borrow(),
                    None,
                ));
                self.pages.push_advance_space();
                self.zvm.handle_save_result(true);
                self.advance()
            }
            Step::Restore => result,
            Step::ReadChar => {
                if force_keyboard {
                    self.pages.show_keyboard = true;
                } else {
                    self.pages.push_advance_space();
                    self.pages.push_element(Element::CharInput);
                }
                result
            }
            _ => {
                self.pages.show_keyboard = false;

                // Add a prompt
                self.pages.maybe_new_page(LINE_HEIGHT * 4);
                self.pages.push_advance_space();
                self.pages.push_element(Element::Input {
                    active: true,
                    contents: UserInput::Ink(vec![]),
                });

                result
            }
        }
    }
}

enum GameState {
    // No game loaded... choose from a list.
    Init { games: Pages },
    // We're in the middle of a game!
    Playing { session: Session },
}

struct Game {
    state: GameState,
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
                        if ext == "z3" || ext == "z4" || ext == "z5" || ext == "z8" {
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

        let ui = BaseUI::new();

        let mut opts = Options::default();
        opts.dimensions.0 = CHARS_PER_LINE as u16;

        let zvm = Zmachine::new(data, ui, opts);

        let dict = Dict(zvm.get_dictionary().into_iter().collect());

        // TODO: get the basename and join to the main root
        // Will allow shipping game files at other file paths in the future
        let save_root = path.with_extension("");
        if !save_root.exists() {
            fs::create_dir(&save_root)?;
        }
        let pages = Pages::new(zvm.unicode_table());
        let session = Session {
            zvm,
            zvm_state: Step::Done,
            dict: Arc::new(dict),
            pages: pages,
            restore: None,
            save_root,
        };

        Ok(session)
    }

    fn game_page(root_dir: &Path) -> Pages {
        let mut games = Pages::new(&[]);

        let header = Text::builder(LINE_HEIGHT * 3, &*BOLD)
            .literal("Folly. ")
            // .font(&*ROMAN)
            // .scale(LINE_HEIGHT as f32)
            // .literal("Handwritten interactive fiction.")
            .into_text();
        games.push_element(Element::Line(false, header));
        // games.push_advance_space();

        let game_vec =
            Game::list_games(root_dir).expect(&format!("Unable to list games in {:?}", root_dir));

        let welcome_message: String = if game_vec.is_empty() {
            format!(
                "It looks like you haven't added any games yet... add a Zcode file to {} and restart the app.",
                root_dir.to_string_lossy(),
            )
        } else {
            "Choose a game from the list to get started.".to_string()
        };

        for widget in Text::wrap(LINE_HEIGHT, &*ROMAN, &welcome_message, LINE_LENGTH, true) {
            games.push_element(Element::Line(false, widget));
        }
        games.push_advance_space();

        let mut elements = vec![];

        for game_path in game_vec {
            let path_str = game_path.to_string_lossy().to_string();
            let slug = game_path
                .file_stem()
                .map_or("unknown".to_string(), |os| os.to_string_lossy().to_string());

            elements.push((slug, game_path, path_str));
        }

        elements.sort_by_key(|(slug, _, _)| slug.to_owned());

        for (slug, path, path_str) in elements {
            games.push_element(Element::file_display(
                &*GAME_ICON,
                &slug,
                &path_str,
                Some(Msg::LoadGame(path)),
            ));
        }
        games
    }

    fn init(ink_tx: mpsc::Sender<(Ink, Arc<Dict>, usize)>, root_dir: PathBuf) -> Game {
        Game {
            state: GameState::Init {
                games: Game::game_page(&root_dir),
            },
            ink_tx,
            awaiting_ink: 0,
            root_dir,
        }
    }

    fn pages_mut(&mut self) -> &mut Pages {
        match &mut self.state {
            GameState::Playing { session, .. } => match &mut session.restore {
                None => &mut session.pages,
                Some(saves) => saves,
            },
            GameState::Init { games } => games,
        }
    }
}

impl Widget for Game {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32)
    }

    fn render(&self, view: View<Msg>) {
        let current_pages = match &self.state {
            GameState::Playing { session, .. } => match &session.restore {
                None => &session.pages,
                Some(saves) => saves,
            },
            GameState::Init { games } => games,
        };

        current_pages.render(view);
    }
}

impl Applet for Game {
    type Upstream = ();

    fn update(&mut self, message: Msg) -> Option<()> {
        match message {
            Msg::Input(ink) => {
                if let GameState::Playing { session } = &mut self.state {
                    if let Some(Element::Input {
                        active: _,
                        contents: UserInput::Ink(existing_ink),
                    }) = &mut session.pages.last_mut().body.last_mut()
                    {
                        existing_ink.push(ink);
                        self.awaiting_ink += 1;
                        let mut merged = Ink::new();
                        for i in existing_ink {
                            merged.append(i.clone(), 0.5);
                        }
                        self.ink_tx
                            .send((merged, session.dict.clone(), self.awaiting_ink))
                            .unwrap();
                    }
                }
            }
            Msg::Submit => {
                if let GameState::Playing { session } = &mut self.state {
                    // TODO: should this submit the string contents also?
                    if let Some(Element::Input {
                        active: _,
                        contents: UserInput::Ink(existing_ink),
                    }) = &mut session.pages.last_mut().body.last_mut()
                    {
                        if existing_ink.len() == 0 {
                            // submit the blank ink!
                            self.awaiting_ink += 1;

                            let mut merged = Ink::new();
                            for i in existing_ink {
                                merged.append(i.clone(), 0.5);
                            }
                            self.ink_tx
                                .send((merged, session.dict.clone(), self.awaiting_ink))
                                .unwrap();
                        }
                    }
                }
            }
            Msg::PageRelative(count) => self.pages_mut().page_relative(count),
            Msg::RecognizedText(n, text) => {
                if let GameState::Playing { session } = &mut self.state {
                    if n == self.awaiting_ink {
                        match session.zvm_state.clone() {
                            Step::ReadChar => {
                                let c = text.chars().next().unwrap_or('\n');
                                session.zvm.handle_read_char(ZChar(c as u8));
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
                                session.restore_menu(saves, false);
                            }
                            Step::Done => {
                                self.state = GameState::Init {
                                    games: Game::game_page(&self.root_dir),
                                };
                            }
                            _ => {}
                        };
                    }
                }
            }
            Msg::LoadGame(game_path) => {
                let mut session = self.load_game(&game_path).unwrap();
                let saves = session.load_saves().unwrap();
                if saves.is_empty() {
                    let state = session.advance();
                    assert!(state == Step::ReadLine || state == Step::ReadChar);
                } else {
                    session.restore_menu(saves, true)
                }

                self.state = GameState::Playing { session }
            }
            Msg::Restore(path, _meta) => {
                if let GameState::Playing { session } = &mut self.state {
                    session.zvm.ui = BaseUI::new();
                    session.restore(&path);
                    let _state = session.advance();
                    // restoring inserts a page break by default, which is boring.
                    session.pages.page_relative(1);
                    session.restore = None;
                }
            }
            Msg::Resume => {
                if let GameState::Playing { session } = &mut self.state {
                    if session.zvm_state == Step::Restore {
                        session.zvm.handle_restore_result();
                    }
                    let _state = session.advance();
                    session.restore = None;
                }
            }
            Msg::ReadChar(zch) => {
                if let GameState::Playing { session } = &mut self.state {
                    if let Some(Element::Input {
                        active: _,
                        contents,
                    }) = &mut session.pages.last_mut().body.last_mut()
                    {
                        match zch {
                            ZChar::ESC => {}
                            ZChar::RETURN => {
                                if let UserInput::String(s) = contents {
                                    session.zvm.handle_input(s.clone());
                                    // FIXME: share this with the read-line code
                                    match session.advance() {
                                        Step::Restore => {
                                            // Start a new page unless the current page is empty
                                            // session.maybe_new_page(TEXT_AREA_HEIGHT);
                                            let saves = session.load_saves().unwrap();
                                            session.restore_menu(saves, false);
                                        }
                                        Step::Done => {
                                            self.state = GameState::Init {
                                                games: Game::game_page(&self.root_dir),
                                            };
                                        }
                                        _ => {}
                                    };
                                }
                            }
                            ZChar::DELETE => {
                                if let UserInput::String(s) = contents {
                                    s.pop();
                                }
                            }
                            _ => {
                                let ch = zch.to_char(session.zvm.unicode_table());
                                match contents {
                                    UserInput::Ink(_) => {
                                        *contents = UserInput::String(ch.to_string())
                                    }
                                    UserInput::String(s) => s.push(ch),
                                }
                            }
                        }
                    } else {
                        session.zvm.handle_read_char(zch);
                        // FIXME: share this with the read-line code
                        match session.advance() {
                            Step::Restore => {
                                // Start a new page unless the current page is empty
                                // session.maybe_new_page(TEXT_AREA_HEIGHT);
                                let saves = session.load_saves().unwrap();
                                session.restore_menu(saves, false);
                            }
                            Step::Done => {
                                self.state = GameState::Init {
                                    games: Game::game_page(&self.root_dir),
                                };
                            }
                            _ => {}
                        };
                    }
                }
            }
            Msg::ToggleKeyboard => {
                let pages = self.pages_mut();
                pages.show_keyboard = !pages.show_keyboard;
                pages.keyboard.shift = 0;

                let show_keyboard = pages.show_keyboard;
                if let Some(Element::Input {
                    active: _,
                    contents,
                }) = &mut pages.last_mut().body.last_mut()
                {
                    *contents = if show_keyboard {
                        UserInput::String(String::new())
                    } else {
                        UserInput::Ink(vec![])
                    };
                }
            }
            Msg::Shift(depth) => {
                let shift = &mut self.pages_mut().keyboard.shift;
                *shift = if *shift == depth { 0 } else { depth };
            }
        }

        None
    }
}

fn main() {
    let root_dir =
        PathBuf::from(std::env::var("FOLLY_ROOT").unwrap_or("/home/root/folly".to_string()));

    if !root_dir.exists() {
        eprintln!(
            "{} does not exist! Creating and prepopulating it.",
            root_dir.to_string_lossy()
        );
        fs::create_dir_all(&root_dir)
            .expect("Unable to (recursively) create the FOLLY_ROOT directory.");
        let mut tutorial_path = root_dir.clone();
        tutorial_path.push("Folly Tutorial.z8");
        fs::write(
            &tutorial_path,
            include_bytes!("../Tutorial.inform/Tutorial.z8"),
        )
        .expect("Unable to write to newly-created directory.");
    }

    let mut app = armrest::app::App::new();

    let (ink_tx, ink_rx) = mpsc::channel::<(Ink, Arc<Dict>, usize)>();
    let wakeup = app.wakeup();

    let mut ink_log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(root_dir.join("ink.log"));

    if let Err(ioerr) = &ink_log {
        eprintln!("Error when opening ink log file: {}", ioerr);
    }

    let mut component = Component::with_sender(wakeup, |text_tx| {
        let _thread = thread::spawn(move || {
            let mut recognizer: Recognizer<Spline> = ml::Recognizer::new().unwrap();

            while let Ok(mut msg) = ink_rx.recv() {
                // We only care about the last message!
                while let Ok(m) = ink_rx.try_recv() {
                    msg = m;
                }

                let (i, dict, n) = msg;

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

        Game::init(ink_tx, root_dir)
    });

    app.run(&mut component);
}
