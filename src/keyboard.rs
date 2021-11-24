use armrest::geom::Side;
use armrest::ui::{Frame, Handlers, Text, Widget};
use encrusted_heart::zscii::ZChar;
use libremarkable::cgmath::Vector2;
use libremarkable::framebuffer::common::DISPLAYWIDTH;
use rusttype::Font;
use std::borrow::Borrow;

const KEY_HEIGHT: i32 = 50;
const LABEL_HEIGHT: i32 = 33;

pub struct Key {
    label: Text,
    width: i32,
    zch: ZChar,
}

pub struct Keyboard {
    keys: Vec<Vec<Key>>,
}

impl Keyboard {
    pub fn new(font: &'static Font<'static>) -> Keyboard {
        let key = |width: i32, label: &str, zch: ZChar| Key {
            label: Text::literal(LABEL_HEIGHT, font, label),
            width,
            zch,
        };

        let keys = vec![
            vec![
                key(80, "Esc", ZChar::ESC),
                key(90, "1", ZChar('1' as u8)),
                key(90, "2", ZChar('2' as u8)),
                key(90, "3", ZChar('3' as u8)),
                key(90, "4", ZChar('4' as u8)),
                key(90, "5", ZChar('5' as u8)),
                key(90, "6", ZChar('6' as u8)),
                key(90, "7", ZChar('7' as u8)),
                key(90, "8", ZChar('8' as u8)),
                key(90, "9", ZChar('9' as u8)),
                key(90, "0", ZChar('0' as u8)),
                key(90, "-", ZChar('-' as u8)),
                key(90, "+", ZChar('+' as u8)),
                key(100, "Delete", ZChar::DELETE),
            ],
            vec![
                key(90, "^", ZChar('\t' as u8)),
                key(90, "q", ZChar('q' as u8)),
                key(90, "w", ZChar('w' as u8)),
                key(90, "e", ZChar('e' as u8)),
                key(90, "r", ZChar('r' as u8)),
                key(90, "t", ZChar('t' as u8)),
                key(90, "y", ZChar('y' as u8)),
                key(90, "u", ZChar('u' as u8)),
                key(90, "i", ZChar('i' as u8)),
                key(90, "o", ZChar('o' as u8)),
                key(90, "p", ZChar('p' as u8)),
                key(90, "[", ZChar('[' as u8)),
                key(90, "]", ZChar(']' as u8)),
                key(90, "\\", ZChar('\\' as u8)),
            ],
            vec![
                key(110, "Special", ZChar(' ' as u8)),
                key(90, "a", ZChar('a' as u8)),
                key(90, "s", ZChar('s' as u8)),
                key(90, "d", ZChar('d' as u8)),
                key(90, "f", ZChar('f' as u8)),
                key(90, "g", ZChar('g' as u8)),
                key(90, "h", ZChar('h' as u8)),
                key(90, "j", ZChar('j' as u8)),
                key(90, "k", ZChar('k' as u8)),
                key(90, "l", ZChar('l' as u8)),
                key(90, ":", ZChar(':' as u8)),
                key(90, "\"", ZChar('\"' as u8)),
                key(160, "Enter", ZChar::RETURN),
            ],
            vec![
                key(130, "Shift", ZChar(' ' as u8)),
                key(90, "z", ZChar('z' as u8)),
                key(90, "x", ZChar('x' as u8)),
                key(90, "c", ZChar('c' as u8)),
                key(90, "v", ZChar('v' as u8)),
                key(90, "b", ZChar('b' as u8)),
                key(90, "n", ZChar('n' as u8)),
                key(90, "m", ZChar('m' as u8)),
                key(90, "<", ZChar('<' as u8)),
                key(90, ">", ZChar('>' as u8)),
                key(90, "?", ZChar('?' as u8)),
                key(230, "Space", ZChar(' ' as u8)),
            ],
        ];

        Keyboard { keys }
    }
}

impl Widget for Keyboard {
    type Message = ZChar;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(1260, KEY_HEIGHT * self.keys.len() as i32)
    }

    fn render<'a>(&'a self, handlers: &'a mut Handlers<Self::Message>, mut frame: Frame<'a>) {
        for row in &self.keys {
            let mut row_frame = frame.split_off(Side::Top, KEY_HEIGHT);
            for (i, key) in row.iter().enumerate() {
                let mut key_frame = row_frame.split_off(Side::Left, key.width);
                handlers.on_tap(&key_frame, key.zch);
                let alignment = if key.width == 90 {
                    0.5
                } else if i == 0 {
                    0.0
                } else {
                    1.0
                };
                key.label
                    .borrow()
                    .void()
                    .render_placed(handlers, key_frame, alignment, 0.5);
            }
        }
    }
}
