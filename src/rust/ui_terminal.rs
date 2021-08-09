#![allow(dead_code)]

use std::boxed::Box;
use std::io;
use std::io::Write;

use atty::Stream;
use term_size;

use crate::traits::{UI, Window};

#[derive(Debug)]
pub struct TerminalUI {
    isatty: bool,
    active_window: Window,
    width: usize,
    x_position: usize,
}

impl TerminalUI {

    pub fn new() -> Box<TerminalUI> {
        if let Some((w, _)) = term_size::dimensions() {
            Box::new(TerminalUI {
                isatty: atty::is(Stream::Stdout),
                active_window: Window::Lower,
                width: w,
                x_position: 0,
            })
        } else {
            Box::new(TerminalUI {
                isatty: false,
                active_window: Window::Lower,
                width: 0,
                x_position: 0,
            })
        }
    }

    pub fn clear(&self) {
        // Clear screen: ESC [2J
        // Move cursor to 1x1: [H
        if self.is_term() {
            self.print_raw("\x1B[2J\x1B[H");
        }
    }

    fn print_raw(&self, raw: &str) {
        print!("{}", raw);
        io::stdout().flush().unwrap();
    }

    fn enter_alternate_screen(&self) {
        if self.is_term() {
            self.print_raw("\x1B[?1049h");
        }
    }

    fn end_alternate_screen(&self) {
        if self.is_term() {
            self.print_raw("\x1B[?1049l");
        }
    }

    fn is_term(&self) -> bool {
        self.isatty
    }
}

impl UI for TerminalUI {
    fn print(&mut self, text: &str) {
        if self.active_window == Window::Upper {
            return;
        }

        if !self.is_term() {
            self.print_raw(text);
            return;
        }

        // `.lines()` discards trailing \n and collapses multiple \n's between lines
        let lines = text.split('\n').collect::<Vec<_>>();
        let num_lines = lines.len();

        // implements some word-wrapping so words don't get split across lines
        lines.iter().enumerate().for_each(|(i, line)| {
            // skip if this line is just the result of a "\n"
            if !line.is_empty() {
                // `.split_whitespace` having similar issues as `.lines` above
                let words = line.split(' ').collect::<Vec<_>>();
                let num_words = words.len();

                // check that each word can fit on the line before printing it.
                // if its too big, bump to the next line and reset x-position
                words.iter().enumerate().for_each(|(i, word)| {
                    self.x_position += word.len();

                    if self.x_position > self.width {
                        self.x_position = word.len();
                        println!();
                    }

                    print!("{}", word);

                    // add spaces back in if we can (an not on the last element)
                    if i < num_words - 1 && self.x_position < self.width {
                        self.x_position += 1;
                        print!(" ");
                    }
                });
            }

            // add newlines back that were removed from split
            if i < num_lines - 1 {
                println!();
                self.x_position = 0;
            }
        });

        io::stdout().flush().unwrap();
    }

    fn debug(&mut self, text: &str) {
        self.print(text);
    }

    fn print_object(&mut self, object: &str) {
        if self.active_window == Window::Upper {
            return;
        }

        if self.is_term() {
            self.print_raw("\x1B[37;1m");
        }
        self.print(object);
        if self.is_term() {
            self.print_raw("\x1B[0m");
        }
    }

    fn set_status_bar(&self, left: &str, right: &str) {
        // ESC ]2; "text" BEL
        if self.is_term() {
            self.print_raw(&format!("\x1B]2;{}  -  {}\x07", left, right));
        }
    }

    fn set_window(&mut self, window: Window) {
        self.active_window = window;
    }

    // unimplemented, only used in web ui
    fn flush(&mut self) {}
    fn message(&self, _mtype: &str, _msg: &str) {}
}
