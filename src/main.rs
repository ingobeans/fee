use std::{
    env::current_dir,
    io::{stdout, Cursor, Result, Stdout, Write},
    path::PathBuf,
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
    fn new(cwd: PathBuf) -> Self {
        Fee {
            listening: false,
            cwd,
            stdout: stdout(),
            selection: 0,
            current_contents: DirectoryContents::new(),
        }
    }
    fn update(&mut self) -> Result<()> {
        queue!(self.stdout, Clear(ClearType::All))?;
        queue!(self.stdout, cursor::MoveTo(0, 0))?;
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
            queue!(self.stdout, ResetColor)?;
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
                break;
            }
            index += 1;
        }
        for file in contents.files {
            if index == self.selection {
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
    fn handle_keypress(&mut self, event: Event) -> Result<()> {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Up => {
                        if self.selection == 0 {
                            self.selection = self.current_contents.count - 1;
                        } else {
                            self.selection -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if self.selection >= self.current_contents.count - 1 {
                            self.selection = 0;
                        } else {
                            self.selection += 1;
                        }
                    }
                    KeyCode::Enter => {
                        self.select()?;
                    }
                    KeyCode::Char(char) => {
                        if char == 'c' && key.modifiers.contains(KeyModifiers::CONTROL) {
                            self.listening = false;
                        }
                    }
                    KeyCode::Esc => {
                        self.go_back()?;
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
        enable_raw_mode()?;
        queue!(self.stdout, cursor::Hide)?;
        self.current_contents = self.get_cwd_contents()?;
        self.update()?;
        while self.listening {
            self.handle_keypress(event::read()?)?;
        }
        disable_raw_mode()?;
        queue!(self.stdout, Clear(ClearType::All))?;
        queue!(self.stdout, cursor::Show)?;
        Ok(())
    }
}

fn main() {
    let cwd = current_dir().unwrap();
    let mut fee = Fee::new(cwd);
    fee.listen().unwrap();
}
