#[derive(Debug)]
pub struct Options {
    pub save_dir: String,
    pub save_name: String,
    pub log_instructions: bool,
    pub rand_seed: [u32; 4],
    pub dimensions: (u16, u16),
    pub undo_limit: usize,
}

impl Options {
    pub fn default() -> Options {
        Options {
            save_dir: String::new(),
            save_name: String::new(),
            log_instructions: false,
            rand_seed: [90, 111, 114, 107],
            dimensions: (80, 255), // 255 is "infinite scrolling"
            undo_limit: 16
        }
    }
}
