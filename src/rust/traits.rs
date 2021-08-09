#[derive(Ord, PartialOrd, Eq, PartialEq, Debug, Copy, Clone)]
pub enum Window {
    Lower,
    Upper,
}

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub struct TextStyle(u16);

impl TextStyle {
    pub(crate) fn new(flags: u16) -> TextStyle { TextStyle(flags) }
    fn roman(self) -> bool { self.0 == 0 }
    fn reverse_video(self) -> bool { self.0 & 0b0001 != 0 }
    fn bold(self) -> bool { self.0 & 0b0010 != 0 }
    fn italic(self) -> bool { self.0 & 0b0100 != 0 }
    fn fixed_pitch(self) -> bool { self.0 & 0b1000 != 0 }
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
