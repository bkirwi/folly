#[derive(Ord, PartialOrd, Eq, PartialEq, Debug, Copy, Clone)]
pub enum Window {
    Lower,
    Upper,
}

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub struct TextStyle(pub u16);

impl TextStyle {
    pub fn new(flags: u16) -> TextStyle {
        TextStyle(flags)
    }
    pub fn roman(self) -> bool {
        self.0 == 0
    }
    pub fn reverse_video(self) -> bool {
        self.0 & 0b0001 != 0
    }
    pub fn bold(self) -> bool {
        self.0 & 0b0010 != 0
    }
    pub fn italic(self) -> bool {
        self.0 & 0b0100 != 0
    }
    pub fn fixed_pitch(self) -> bool {
        self.0 & 0b1000 != 0
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle(0)
    }
}

pub trait UI {
    fn print(&mut self, text: &str, style: TextStyle);
    fn debug(&mut self, text: &str);
    fn print_object(&mut self, object: &str);
    fn set_status_bar(&mut self, left: &str, right: &str);

    fn split_window(&mut self, _lines: u16) {}
    fn set_window(&mut self, _window: Window) {}
    fn erase_window(&mut self, _window: Window) {}
    fn set_cursor(&mut self, _line: u16, _column: u16) {}
    // fn set_text_style(&mut self, _text_style: TextStyle) {}

    // only used by web ui
    fn flush(&mut self);
    fn message(&self, mtype: &str, msg: &str);
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct BaseOutput {
    pub style: TextStyle,
    pub content: String,
}

pub struct BaseUI {
    current_window: Window,
    upper_cursor: (usize, usize),
    upper_lines: Vec<Vec<(TextStyle, char)>>,
    requested_height: usize, // https://eblong.com/zarf/glk/quote-box.html
    cleared: bool,
    output: Vec<BaseOutput>,
    status_line: Option<(String, String)>,
}

impl BaseUI {
    pub fn new() -> BaseUI {
        BaseUI {
            current_window: Window::Lower,
            upper_cursor: (0, 0),
            upper_lines: vec![],
            requested_height: 0,
            cleared: true,
            output: vec![],
            status_line: None,
        }
    }

    pub fn is_cleared(&self) -> bool {
        self.cleared
    }

    pub fn status_line(&self) -> Option<(&str, &str)> {
        self.status_line
            .as_ref()
            .map(|(l, r)| (l.as_str(), r.as_str()))
    }

    pub fn resolve_upper_height(&mut self) {
        self.upper_lines.truncate(self.requested_height);
    }

    pub fn upper_window(&self) -> &Vec<Vec<(TextStyle, char)>> {
        &self.upper_lines
    }

    pub fn drain_output(&mut self) -> Vec<BaseOutput> {
        self.cleared = false;
        std::mem::take(&mut self.output)
    }
}

impl UI for BaseUI {
    fn print(&mut self, text: &str, style: TextStyle) {
        match self.current_window {
            Window::Lower => match self.output.last_mut() {
                Some(BaseOutput {
                    style: old_style,
                    content,
                }) if *old_style == style => {
                    content.push_str(text);
                }
                _ => self.output.push(BaseOutput {
                    style,
                    content: text.to_string(),
                }),
            },
            Window::Upper => {
                self.resolve_upper_height();
                for c in text.chars() {
                    let (line_number, column_number) = self.upper_cursor;

                    if c == '\n' {
                        self.upper_cursor = (line_number + 1, 0);
                        continue;
                    }

                    let window_len = self.upper_lines.len();
                    if window_len <= line_number {
                        self.split_window(1 + line_number as u16);
                    }
                    let line = &mut self.upper_lines[line_number];
                    // If the cursor is past the end of the line, pad it out with blanks.
                    for _ in line.len()..=column_number {
                        line.push((TextStyle::new(0), ' '));
                    }
                    line[column_number] = (style, c);
                    self.upper_cursor.1 += 1
                }
            }
        }
    }

    fn debug(&mut self, text: &str) {
        eprintln!("Debug text: {}", text)
    }

    fn print_object(&mut self, object: &str) {
        self.print(object, TextStyle::default());
    }

    fn set_status_bar(&mut self, left: &str, right: &str) {
        self.status_line = Some((left.to_string(), right.to_string()));
    }

    fn split_window(&mut self, lines: u16) {
        let current_lines = self.requested_height;
        let requested_lines = lines as usize;

        if current_lines < requested_lines {
            self.resolve_upper_height();
            for _ in current_lines..requested_lines {
                self.upper_lines.push(vec![]);
            }
        }
        self.requested_height = requested_lines;
    }

    fn set_window(&mut self, window: Window) {
        if self.current_window == window {
            return;
        }
        self.current_window = window;

        if window == Window::Upper {
            self.upper_cursor = (0, 0);
        }
    }

    fn erase_window(&mut self, window: Window) {
        match window {
            Window::Lower => {
                self.cleared = true;
                self.output.clear();
            }
            Window::Upper => {
                for line in &mut self.upper_lines {
                    line.clear();
                }
            }
        }
    }

    fn set_cursor(&mut self, line: u16, column: u16) {
        if self.current_window == Window::Upper {
            // If this is out of bounds, we'll fix it in `print`.
            fn to_index(i: u16) -> usize {
                i.saturating_sub(1) as usize
            }
            self.upper_cursor = (to_index(line), to_index(column));
        }
    }

    fn flush(&mut self) {}

    fn message(&self, _mtype: &str, _msg: &str) {}
}
