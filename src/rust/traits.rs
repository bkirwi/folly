#[derive(Ord, PartialOrd, Eq, PartialEq, Debug, Copy, Clone)]
pub enum Window {
    Lower,
    Upper,
}

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub struct TextStyle(u16);

impl TextStyle {
    pub fn new(flags: u16) -> TextStyle { TextStyle(flags) }
    pub fn roman(self) -> bool { self.0 == 0 }
    pub fn reverse_video(self) -> bool { self.0 & 0b0001 != 0 }
    pub fn bold(self) -> bool { self.0 & 0b0010 != 0 }
    pub fn italic(self) -> bool { self.0 & 0b0100 != 0 }
    pub fn fixed_pitch(self) -> bool { self.0 & 0b1000 != 0 }
}

pub trait UI {
    fn print(&mut self, text: &str);
    fn debug(&mut self, text: &str);
    fn print_object(&mut self, object: &str);
    fn set_status_bar(&self, left: &str, right: &str);

    fn split_window(&mut self, _lines: u16) {}
    fn set_window(&mut self, _window: Window) {}
    fn erase_window(&mut self, _window: Window) {}
    fn set_cursor(&mut self, _line: u16, _column: u16) {}
    fn set_text_style(&mut self, _text_style: TextStyle) {}

    // only used by web ui
    fn flush(&mut self);
    fn message(&self, mtype: &str, msg: &str);
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub enum BaseOutput {
    Upper {
        lines: Vec<Vec<(TextStyle, char)>>
    },
    Lower {
        style: TextStyle,
        content: String,
    },
    EraseLower,
}

pub struct BaseUI {
    current_window: Window,
    current_style: TextStyle,
    upper_cursor: (usize, usize),
    upper_lines: Vec<Vec<(TextStyle, char)>>,
    requested_height: usize, // https://eblong.com/zarf/glk/quote-box.html
    output: Vec<BaseOutput>,
}

impl BaseUI {
    pub fn new() -> BaseUI {
        BaseUI {
            current_window: Window::Lower,
            current_style: TextStyle::new(0),
            upper_cursor: (0, 0),
            upper_lines: vec![],
            requested_height: 0,
            output: vec![],
        }
    }

    pub fn resolve_upper_height(&mut self) {
        self.upper_lines.truncate(self.requested_height);
    }

    pub fn upper_window(&self) -> &Vec<Vec<(TextStyle, char)>> {
        &self.upper_lines
    }

    pub fn drain_output(&mut self) -> Vec<BaseOutput> {
        if self.current_window == Window::Upper {
            self.output.push(BaseOutput::Upper { lines: self.upper_lines.clone() })
        }

        std::mem::take(&mut self.output)
    }
}


impl UI for BaseUI {
    fn print(&mut self, text: &str) {
        match self.current_window {
            Window::Lower => {
                match self.output.last_mut() {
                    Some(BaseOutput::Lower { style, content })
                    if *style == self.current_style => {
                        content.push_str(text);
                    }
                    _ => {
                        self.output.push(BaseOutput::Lower {
                            style: self.current_style,
                            content: text.to_string()
                        })
                    }
                }
            }
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
                    line[column_number] = (self.current_style, c);
                    self.upper_cursor.1 += 1
                }
            }
        }
    }

    fn debug(&mut self, text: &str) {
        eprintln!("Debug text: {}", text)
    }

    fn print_object(&mut self, object: &str) {
        self.print(object);
    }

    fn set_status_bar(&self, left: &str, right: &str) {
    }

    fn split_window(&mut self, lines: u16) {
        let current_lines = self.upper_lines.len();
        let requested_lines = lines as usize;
        self.requested_height = requested_lines;
        if current_lines > requested_lines {
            self.output.push(BaseOutput::Upper { lines: self.upper_lines.clone() });
        } else if current_lines < requested_lines {
            for _ in current_lines..requested_lines {
                self.upper_lines.push(vec![]);
            }
        }
    }

    fn set_window(&mut self, window: Window) {
        if self.current_window == window {
            return;
        }
        self.current_window = window;

        match window {
            Window::Lower => {
                self.output.push(BaseOutput::Upper { lines: self.upper_lines.clone() });
            }
            Window::Upper => {
                self.upper_cursor = (0, 0);
            }
        }
    }

    fn erase_window(&mut self, window: Window) {
        match window {
            Window::Lower => {
                self.output.push(BaseOutput::EraseLower);
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

    fn set_text_style(&mut self, text_style: TextStyle) {
        self.current_style = text_style;
    }

    fn flush(&mut self) {}

    fn message(&self, mtype: &str, msg: &str) {}
}
