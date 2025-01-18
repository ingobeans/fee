use std::{
    collections::VecDeque,
    env::current_dir,
    io::{stdout, Read, Result, Stdout, Write},
    path::PathBuf,
    process::Command,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    queue,
    style::{Color, ResetColor, SetBackgroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};

struct Fee {
    listening: bool,
    cwd: PathBuf,
    config: Config,
    stdout: Stdout,
    selection: u16,
    current_contents: DirectoryContents,
}
#[derive(Clone)]
struct Directory(String);
#[derive(Clone)]
struct File(String);

#[derive(Clone)]
struct DirectoryContents {
    dirs: Vec<Directory>,
    files: Vec<File>,
    count: u16,
}
impl DirectoryContents {
    fn new() -> Self {
        DirectoryContents {
            dirs: vec![],
            files: vec![],
            count: 0,
        }
    }
    fn from(dirs: Vec<Directory>, files: Vec<File>) -> Self {
        let count = (dirs.len() + files.len()) as u16;
        DirectoryContents { dirs, files, count }
    }
}

impl Fee {
    fn new(cwd: PathBuf, config: Config) -> Self {
        Fee {
            listening: false,
            cwd,
            config,
            stdout: stdout(),
            selection: 0,
            current_contents: DirectoryContents::new(),
        }
    }
    fn cleanup_terminal(&mut self) -> Result<()> {
        queue!(
            self.stdout,
            Clear(ClearType::All),
            cursor::Show,
            cursor::MoveTo(0, 0),
            ResetColor
        )?;
        self.stdout.flush()?;
        disable_raw_mode()?;
        Ok(())
    }
    fn prepare_terminal(&mut self) -> Result<()> {
        self.stdout = stdout();
        self.stdout.flush()?;
        queue!(
            self.stdout,
            Clear(ClearType::All),
            cursor::Hide,
            cursor::MoveTo(0, 0)
        )?;
        self.stdout.flush()?;
        enable_raw_mode()?;
        self.current_contents = self.get_cwd_contents()?;
        Ok(())
    }
    fn update(&mut self) -> Result<()> {
        queue!(
            self.stdout,
            Clear(ClearType::All),
            cursor::MoveTo(0, 0),
            ResetColor
        )?;
        self.draw_text()?;
        queue!(self.stdout, cursor::MoveTo(0, 0))?;
        self.stdout.flush()?;
        Ok(())
    }
    fn get_cwd_contents(&self) -> Result<DirectoryContents> {
        let mut dirs = vec![];
        let mut files = vec![];

        for item in std::fs::read_dir(&self.cwd)?.flatten() {
            let item_type = item.file_type()?;
            let item_name = item
                .file_name()
                .to_str()
                .ok_or(std::io::Error::other("Couldn't get filename of item."))?
                .to_string();

            if item_type.is_dir() {
                dirs.push(Directory(item_name))
            } else if item_type.is_file() {
                files.push(File(item_name))
            }
        }

        Ok(DirectoryContents::from(dirs, files))
    }

    fn print_line(&mut self, text: &str, x: u16, y: u16, highlighted: bool) -> Result<()> {
        queue!(self.stdout, cursor::MoveTo(x, y))?;
        if highlighted {
            queue!(self.stdout, SetBackgroundColor(Color::White))?;
        }
        print!("{}", text);
        if highlighted {
            queue!(self.stdout, SetBackgroundColor(Color::Reset))?;
        }
        Ok(())
    }
    fn draw_text(&mut self) -> Result<()> {
        let contents = self.current_contents.clone();
        let mut index = 0;
        for dir in contents.dirs {
            self.print_line(&dir.0, 0, index, self.selection == index)?;
            index += 1;
        }
        for file in contents.files {
            self.print_line(&file.0, 0, index, self.selection == index)?;
            index += 1;
        }
        Ok(())
    }
    fn select(&mut self) -> Result<()> {
        let contents = self.current_contents.clone();
        let mut index = 0;
        for dir in contents.dirs {
            if index == self.selection {
                self.cwd.push(&dir.0);
                self.selection = 0;
                self.current_contents = self.get_cwd_contents()?;
                return Ok(());
            }
            index += 1;
        }
        for file in contents.files {
            if index == self.selection {
                let mut filepath = self.cwd.clone();
                filepath.push(&file.0);

                let mut parts: VecDeque<String> = [].into();
                let mut command = &self.config.text_editor_command;
                if self.config.text_editor_command != self.config.binary_editor_command {
                    // if the binary editor != the text editor
                    // check if the file is utf-8 or if it should be read with the binary editor
                    if !is_valid_utf8(&filepath)? {
                        command = &self.config.binary_editor_command;
                    }
                }

                let filepath_str = filepath
                    .to_str()
                    .ok_or(std::io::Error::other("Couldn't convert path to str."))?;

                for part in command {
                    if part == "$f" {
                        parts.push_back(filepath_str.to_string());
                    } else {
                        parts.push_back(part.to_string());
                    }
                }

                let first = parts.pop_front();
                match first {
                    Some(executable) => {
                        let mut command = Command::new(executable);
                        command.args(parts);
                        self.cleanup_terminal()?;
                        if self.config.wait_for_editor_exit {
                            command.spawn()?.wait()?;
                            self.prepare_terminal()?;
                            self.update()?;
                        } else {
                            command.spawn()?;
                            self.prepare_terminal()?;
                            self.update()?;
                        }
                    }
                    None => {
                        println!("No");
                    }
                }
                break;
            }
            index += 1;
        }
        Ok(())
    }
    fn go_back(&mut self) -> Result<()> {
        let parent = self.cwd.parent();
        if let Some(parent) = parent {
            self.cwd = parent.to_path_buf();
            self.selection = 0;
            self.current_contents = self.get_cwd_contents()?;
        }
        Ok(())
    }
    fn move_up(&mut self) {
        if self.selection == 0 {
            self.selection = self.current_contents.count - 1;
        } else {
            self.selection -= 1;
        }
    }
    fn move_down(&mut self) {
        if self.selection >= self.current_contents.count - 1 {
            self.selection = 0;
        } else {
            self.selection += 1;
        }
    }
    fn handle_keypress(&mut self, event: Event) -> Result<()> {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Up => self.move_up(),
                    KeyCode::Down => self.move_down(),
                    KeyCode::Enter => self.select()?,
                    KeyCode::Right => self.select()?,
                    KeyCode::Esc => self.go_back()?,
                    KeyCode::Left => self.go_back()?,
                    KeyCode::Char(char) => {
                        if char == 'c' && key.modifiers.contains(KeyModifiers::CONTROL) {
                            self.listening = false;
                        }
                    }
                    _ => {}
                }
                self.update()?;
            }
        }
        Ok(())
    }

    fn listen(&mut self) -> Result<()> {
        self.listening = true;
        self.prepare_terminal()?;
        self.update()?;
        while self.listening {
            self.handle_keypress(event::read()?)?;
        }
        self.cleanup_terminal()?;
        Ok(())
    }
}

struct Config {
    text_editor_command: Vec<String>,
    binary_editor_command: Vec<String>,
    wait_for_editor_exit: bool,
}
fn is_valid_utf8(path: &PathBuf) -> Result<bool> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0; 128];
    let mut offset: isize = 0;
    loop {
        let bytes_read = file.read(&mut buf[offset as usize..])?;
        if bytes_read == 0 {
            return Ok(offset == 0);
        }
        match std::str::from_utf8(&buf[..(offset + bytes_read as isize) as usize]) {
            Ok(_) => offset = 0,
            Err(e) if e.error_len().is_some() => return Ok(false),
            Err(e) => {
                buf.copy_within(e.valid_up_to()..(offset + bytes_read as isize) as usize, 0);
                offset += bytes_read as isize - e.valid_up_to() as isize;
            }
        }
    }
}

fn main() {
    let cwd = current_dir().unwrap();
    let config = Config {
        text_editor_command: vec!["banano".to_string(), "$f".to_string()],
        binary_editor_command: vec!["hexed".to_string(), "$f".to_string()],
        wait_for_editor_exit: true,
    };

    let mut fee = Fee::new(cwd, config);
    fee.listen().unwrap();
}
