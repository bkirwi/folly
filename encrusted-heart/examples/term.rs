extern crate base64;
extern crate clap;
extern crate encrusted_heart;
#[macro_use]
extern crate lazy_static;
extern crate rand;
extern crate regex;
extern crate serde_json;
extern crate termion;

use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::{io, mem, process};

use clap::{App, Arg};
use regex::Regex;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

use encrusted_heart::options::Options;
use encrusted_heart::traits::{BaseOutput, BaseUI};
use encrusted_heart::zmachine::{Step, Zmachine};
use encrusted_heart::zscii::ZChar;

const VERSION: &str = env!("CARGO_PKG_VERSION");

lazy_static! {
    static ref ANSI_RE: Regex =
        Regex::new(r"[\x1b\x9b][\[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-PRZcf-nqry=><]")
            .unwrap();
}

fn get_user_input() -> Option<String> {
    let mut input = String::new();
    let result = io::stdin()
        .read_line(&mut input)
        .expect("Error reading input");

    // trim, strip and control sequences that might have gotten in,
    // and then trim once more to get rid of any excess whitespace
    let trimmed = ANSI_RE
        .replace_all(input.trim(), "")
        .to_string()
        .trim()
        .to_string();

    if result == 0 {
        None
    } else {
        Some(trimmed)
    }
}

fn main() {
    let is_tty = termion::is_tty(&io::stdout().lock());

    let matches = App::new("encrusted")
        .version(VERSION)
        .about("A zmachine interpreter")
        .arg(
            Arg::with_name("FILE")
                .help("Sets the story file to run")
                .required(true),
        )
        .arg(
            Arg::with_name("debug")
                .long("debug")
                .takes_value(false)
                .help("Log verbose debugging information to stderr"),
        )
        .get_matches();

    let path = Path::new(matches.value_of("FILE").unwrap());

    let debug_log = matches.is_present("debug");

    if !path.is_file() {
        println!(
            "\nCouldn't find game file: \n   {}\n",
            path.to_string_lossy()
        );
        process::exit(1);
    }

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

    let ui = BaseUI::new();

    let (term_width, term_height) = if is_tty {
        termion::terminal_size().unwrap()
    } else {
        (80, 24)
    };

    let mut opts = Options::default();
    let save_dir = path.parent().unwrap().to_string_lossy().into_owned();
    let mut save_name = path.file_stem().unwrap().to_string_lossy().into_owned();

    let rand32 = || rand::random();
    opts.rand_seed = [rand32(), rand32(), rand32(), rand32()];
    opts.dimensions = (term_width, term_height);
    opts.log_instructions = debug_log;

    if is_tty {
        print!("{}", termion::clear::All);
    }

    let mut zvm = Zmachine::new(data, ui, opts);
    let mut x_position = 0;

    loop {
        let step = zvm.step();

        if is_tty && zvm.ui.is_cleared() {
            print!("{}", termion::clear::All);
        }
        for BaseOutput {
            style: _,
            content: text,
        } in zvm.ui.drain_output()
        {
            if !is_tty {
                print!("{}", &text);
                io::stdout().flush().unwrap();
                continue;
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
                        x_position += word.len();

                        if x_position > term_width as usize {
                            x_position = word.len();
                            println!();
                        }

                        print!("{}", word);

                        // add spaces back in if we can (an not on the last element)
                        if i < num_words - 1 && x_position < term_width as usize {
                            x_position += 1;
                            print!(" ");
                        }
                    });
                }

                // add newlines back that were removed from split
                if i < num_lines - 1 {
                    println!();
                    x_position = 0;
                }
            });

            io::stdout().flush().unwrap();
        }

        if is_tty {
            print!("{}", termion::cursor::Save);
            // eprintln!();
            for (line_no, chars) in zvm.ui.upper_window().clone().into_iter().enumerate() {
                print!(
                    "{}{}",
                    termion::cursor::Goto(1, 1 + line_no as u16),
                    termion::clear::CurrentLine
                );
                for (style, c) in chars.into_iter().take(term_width as usize) {
                    if style.bold() {
                        print!("{}", termion::style::Bold);
                    }
                    if style.italic() {
                        print!("{}", termion::style::Italic);
                    }
                    if style.reverse_video() {
                        print!("{}", termion::style::Invert);
                    }
                    print!("{}{}", c, termion::style::Reset);
                }
            }
            print!("{}", termion::cursor::Restore);
            io::stdout().flush().unwrap();
        }

        match step {
            Step::Done => {
                break;
            }
            Step::Save(data) => {
                println!("\nFilename [{}]: ", &save_name);
                let input = get_user_input().unwrap();
                let mut path = PathBuf::from(&save_dir);
                let mut file;

                match input.to_lowercase().as_ref() {
                    "" | "yes" | "y" => path.push(&save_name),
                    "no" | "n" | "cancel" => {
                        zvm.handle_save_result(false);
                        continue;
                    }
                    _ => path.push(input),
                }

                if let Ok(handle) = File::create(&path) {
                    file = handle;
                } else {
                    println!("Can't save to that file, try another?\n");
                    zvm.handle_save_result(false);
                    return;
                }

                // save file name for next use
                save_name = path.file_name().unwrap().to_string_lossy().into_owned();

                // The save PC points to either the save instructions branch data or store
                // data. In either case, this is the last byte of the instruction. (so -1)
                file.write_all(data.as_slice())
                    .expect("Error saving to file");

                zvm.handle_save_result(true);
            }
            Step::Restore => {
                let mut file = loop {
                    print!("\nFilename [{}]: ", &save_name);
                    io::stdout().flush().unwrap();

                    let input = get_user_input().unwrap();
                    let path = PathBuf::from(&input);
                    match File::open(&path) {
                        Ok(h) => break h,
                        Err(e) => println!("Can't open file: {:?}", e),
                    };
                };

                let mut save_data = vec![];

                // restore program counter position, stack frames, and dynamic memory
                file.read_to_end(&mut save_data)
                    .expect("Error reading save file");
                zvm.restore(&save_data);
            }
            Step::ReadChar => {
                let zscii: ZChar = if is_tty {
                    let stdout = io::stdout().into_raw_mode().unwrap();
                    let mut keys = io::stdin().keys();

                    // While we expect just a single char, this loops in case unexpected characters
                    // are encountered. (We ignore them.)
                    let zch = loop {
                        let key = keys.next().expect("Error reading input").unwrap();
                        break match key {
                            Key::Backspace => ZChar::DELETE,
                            Key::Delete => ZChar::DELETE,
                            Key::Esc => ZChar::ESC,
                            Key::Up => ZChar::UP,
                            Key::Down => ZChar::DOWN,
                            Key::Left => ZChar::LEFT,
                            Key::Right => ZChar::RIGHT,
                            Key::Char(c) => match ZChar::from_char(c, zvm.unicode_table()) {
                                None => continue,
                                Some(zch) => zch,
                            },
                            _ => continue,
                        };
                    };
                    mem::drop(stdout);
                    zch
                } else {
                    ZChar::RETURN
                };

                zvm.handle_read_char(zscii);
            }
            Step::ReadLine => {
                let input = match get_user_input() {
                    None => return,
                    Some(line) => line,
                };
                if !is_tty {
                    // This won't be automatically visible in stdout!
                    println!("{}", &input);
                }
                zvm.handle_input(input);
            }
        }
    }

    println!()
}
