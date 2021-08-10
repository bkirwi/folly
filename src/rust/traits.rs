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
    upper_cursor: (u16, u16),
    upper_lines: Vec<Vec<(TextStyle, char)>>,
    output: Vec<BaseOutput>,
}

impl BaseUI {
    pub(crate) fn new() -> BaseUI {
        BaseUI {
            current_window: Window::Lower,
            current_style: TextStyle::new(0),
            upper_cursor: (0, 0),
            upper_lines: vec![],
            output: vec![],
        }
    }

    pub fn drain_output(&mut self) -> Vec<BaseOutput> {
        // TODO: maybe there's something to do here?
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
                let line_number = self.upper_cursor.0 as usize;
                let window_len = self.upper_lines.len();
                if window_len <= line_number {
                    self.split_window(self.upper_cursor.0 + 1);
                }

                let line = &mut self.upper_lines[line_number];
                let mut index = self.upper_cursor.1 as usize;

                for c in text.chars() {
                    for i in line.len()..=index {
                        line.push((TextStyle::new(0), ' '));
                    }
                    line[index] = (self.current_style, c);
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
        if current_lines > requested_lines {
            self.output.push(BaseOutput::Upper { lines: self.upper_lines.clone() });
            self.upper_lines.truncate(requested_lines);
        } else if current_lines < requested_lines {
            for _ in current_lines..requested_lines {
                self.upper_lines.push(vec![]);
            }
        }
    }

    fn set_window(&mut self, window: Window) {
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
            self.upper_cursor = (line, column);
        }
    }

    fn set_text_style(&mut self, text_style: TextStyle) {
        self.current_style = text_style;
    }

    fn flush(&mut self) {}

    fn message(&self, mtype: &str, msg: &str) {}
}
