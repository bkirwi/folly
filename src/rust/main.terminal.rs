extern crate atty;
extern crate base64;
extern crate clap;
extern crate rand;
extern crate regex;
extern crate serde_json;
extern crate term_size;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate enum_primitive;

#[macro_use]
extern crate serde_derive;

use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::{process, io};

use clap::{App, Arg};

mod buffer;
mod frame;
mod instruction;
mod options;
mod quetzal;
mod traits;
mod ui_terminal;
mod zmachine;

use crate::options::Options;
use crate::traits::UI;
use crate::ui_terminal::TerminalUI;
use crate::zmachine::{Zmachine, Step};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let matches = App::new("encrusted")
        .version(VERSION)
        .about("A zmachine interpreter")
        .arg(
            Arg::with_name("FILE")
                .help("Sets the story file to run")
                .required(true),
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

    let ui = TerminalUI::new();

    let mut opts = Options::default();
    opts.save_dir = path.parent().unwrap().to_string_lossy().into_owned();
    opts.save_name = path.file_stem().unwrap().to_string_lossy().into_owned();

    let rand32 = || rand::random();
    opts.rand_seed = [rand32(), rand32(), rand32(), rand32()];

    ui.clear();

    let mut zvm = Zmachine::new(data, ui, opts);

    loop {
        match zvm.step() {
            Step::Done => {
                break;
            }
            Step::Save(data) => {
                let prompt = format!("\nFilename [{}]: ", zvm.options.save_name);
                zvm.ui.print(&prompt);

                let input = zvm.ui.get_user_input();
                let mut path = PathBuf::from(&zvm.options.save_dir);
                let mut file;

                match input.to_lowercase().as_ref() {
                    "" | "yes" | "y" => path.push(&zvm.options.save_name),
                    "no" | "n" | "cancel" => {
                        zvm.handle_save_result(false);
                        continue;
                    }
                    _ => path.push(input),
                }

                if let Ok(handle) = File::create(&path) {
                    file = handle;
                } else {
                    zvm.ui.print("Can't save to that file, try another?\n");
                    zvm.handle_save_result(false);
                    return;
                }

                // save file name for next use
                zvm.options.save_name = path.file_name().unwrap().to_string_lossy().into_owned();

                // The save PC points to either the save instructions branch data or store
                // data. In either case, this is the last byte of the instruction. (so -1)
                file.write_all(data.as_slice()).expect("Error saving to file");

                zvm.handle_save_result(true);
            }
            Step::Restore => {
                let prompt = format!("\nFilename [{}]: ", &zvm.options.save_name);
                zvm.ui.print(&prompt);

                let input = zvm.ui.get_user_input();
                let mut path = PathBuf::from(&zvm.options.save_dir);
                let mut data = Vec::new();
                let mut file;

                match input.to_lowercase().as_ref() {
                    "" | "yes" | "y" => path.push(&zvm.options.save_name),
                    "no" | "n" | "cancel" => {
                        panic!("ugh");
                    }
                    _ => path.push(input),
                }

                if let Ok(handle) = File::open(&path) {
                    file = handle;
                } else {
                    panic!("Can't open that file, try another?\n");
                }

                // save file name for next use
                zvm.options.save_name = path.file_name().unwrap().to_string_lossy().into_owned();

                // restore program counter position, stack frames, and dynamic memory
                file.read_to_end(&mut data).expect(
                    "Error reading save file",
                );
                zvm.restore(&base64::encode(&data));
            }
            Step::ReadChar => {
                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .expect("Error reading input");
                let ch = input.chars().next().unwrap_or('\n');
                zvm.handle_read_char(ch);
            }
            Step::ReadLine => {
                let input = zvm.ui.get_user_input();
                zvm.handle_input(input);
            }
        }
    }

    println!()
}
