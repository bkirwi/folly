use armrest::geom::Side;
use armrest::ui::{Frame, Handlers, Text, Widget};
use encrusted_heart::zscii::ZChar;
use libremarkable::cgmath::Vector2;
use libremarkable::framebuffer::common::DISPLAYWIDTH;
use rusttype::Font;
use std::borrow::Borrow;

const KEY_HEIGHT: i32 = 80;
const LABEL_HEIGHT: i32 = 33;

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub enum KeyPress {
    ZChar(ZChar),
    Shift(usize),
}

pub struct Key {
    width: i32,
    special: bool,
    chars: Vec<(Text, KeyPress)>,
}

pub struct Keyboard {
    keys: Vec<Vec<Key>>,
    pub shift: usize,
}

impl Keyboard {
    pub fn new(font: &'static Font<'static>, special: &[char]) -> Keyboard {
        let mut special_iter = special.iter();

        let mut label = |label: &str| Text::literal(LABEL_HEIGHT, font, label);

        let mut key = |char_slice: &[char]| {
            let special_ch = special_iter.next();
            let chars = char_slice
                .iter()
                .chain(special_ch.into_iter())
                .map(|ch| {
                    let text = label(&ch.to_string());
                    let zch = ZChar::from_char(*ch, special).expect("Illegal char in constructor!");
                    (text, KeyPress::ZChar(zch))
                })
                .collect();

            Key {
                width: 90,
                special: false,
                chars,
            }
        };

        fn letter(ch: char) -> [char; 2] {
            [ch, ch.to_ascii_uppercase()]
        };

        let keys = vec![
            vec![
                Key {
                    width: 80,
                    special: true,
                    chars: vec![(label("Esc"), KeyPress::ZChar(ZChar::ESC))],
                },
                key(&['1', '!']),
                key(&['2', '@']),
                key(&['3', '#']),
                key(&['4', '$']),
                key(&['5', '%']),
                key(&['6', '^']),
                key(&['7', '&']),
                key(&['8', '*']),
                key(&['9', '(']),
                key(&['0', ')']),
                key(&['-', '_']),
                key(&['+', '=']),
                Key {
                    width: 100,
                    special: true,
                    chars: vec![(label("Delete"), KeyPress::ZChar(ZChar::DELETE))],
                },
            ],
            vec![
                Key {
                    width: 90,
                    special: true,
                    chars: vec![(label("Tab"), KeyPress::ZChar(ZChar('\t' as u8)))],
                },
                key(&letter('q')),
                key(&letter('w')),
                key(&letter('e')),
                key(&letter('r')),
                key(&letter('t')),
                key(&letter('y')),
                key(&letter('u')),
                key(&letter('i')),
                key(&letter('o')),
                key(&letter('p')),
                key(&['[', '{']),
                key(&[']', '}']),
                key(&['\\', '|']),
            ],
            vec![
                Key {
                    width: 110,
                    special: true,
                    chars: vec![(label("Special"), KeyPress::Shift(2))],
                },
                key(&letter('a')),
                key(&letter('s')),
                key(&letter('d')),
                key(&letter('f')),
                key(&letter('g')),
                key(&letter('h')),
                key(&letter('j')),
                key(&letter('k')),
                key(&letter('l')),
                key(&[';', ':']),
                key(&['/', '?']),
                Key {
                    width: 160,
                    special: true,
                    chars: vec![(label("Return"), KeyPress::ZChar(ZChar::RETURN))],
                },
            ],
            vec![
                Key {
                    width: 130,
                    special: true,
                    chars: vec![(label("Shift"), KeyPress::Shift(1))],
                },
                key(&letter('z')),
                key(&letter('x')),
                key(&letter('c')),
                key(&letter('v')),
                key(&letter('b')),
                key(&letter('n')),
                key(&letter('m')),
                key(&[',', '<']),
                key(&['.', '>']),
                key(&['/', '?']),
                Key {
                    width: 230,
                    special: true,
                    chars: vec![(label("Space"), KeyPress::ZChar(ZChar(' ' as u8)))],
                },
            ],
        ];

        Keyboard { keys, shift: 0 }
    }
}

impl Widget for Keyboard {
    type Message = KeyPress;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(1260, KEY_HEIGHT * self.keys.len() as i32)
    }

    fn render<'a>(&'a self, handlers: &'a mut Handlers<Self::Message>, mut frame: Frame<'a>) {
        let row_height = (frame.size().y / self.keys.len() as i32).min(KEY_HEIGHT);
        for row in &self.keys {
            let mut row_frame = frame.split_off(Side::Top, row_height);
            for (i, key) in row.iter().enumerate() {
                let mut key_frame = row_frame.split_off(Side::Left, key.width);
                let shift = if key.special { 0 } else { self.shift };
                if let Some((label, press)) = key.chars.get(shift) {
                    handlers.on_tap(&key_frame, press.clone());
                    let alignment = if !key.special {
                        0.5
                    } else if i == 0 {
                        0.0
                    } else {
                        1.0
                    };

                    label
                        .borrow()
                        .void()
                        .render_placed(handlers, key_frame, alignment, 0.2);
                }
            }
        }
    }
}
