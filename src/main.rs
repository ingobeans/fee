use std::{
    cmp,
    collections::VecDeque,
    env::current_dir,
    io::{self, stdout, Error, Read, Stdout, Write},
    path::{Path, PathBuf},
    process::Command,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    queue,
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};
use dirs::config_dir;
use serde::{Deserialize, Serialize};

enum ItemType {
    File,
    Directory,
}

struct Item {
    name: String,
    item_type: ItemType,
}
impl Item {
    fn _is_dir(&self) -> bool {
        matches!(self.item_type, ItemType::Directory)
    }
    fn is_file(&self) -> bool {
        matches!(self.item_type, ItemType::File)
    }
}

struct Fee {
    listening: bool,
    cwd: PathBuf,
    config: Config,
    stdout: Stdout,
    selection: u16,
    scroll: u16,
    current_contents: Vec<Item>,
}
impl Fee {
    fn new(cwd: PathBuf, config: Config) -> Self {
        Fee {
            listening: false,
            cwd,
            config,
            stdout: stdout(),
            selection: 0,
            scroll: 0,
            current_contents: vec![],
        }
    }
    fn cleanup_terminal(&mut self) -> io::Result<()> {
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
    fn prepare_terminal(&mut self) -> io::Result<()> {
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
    fn update(&mut self) -> io::Result<()> {
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
    fn get_cwd_contents(&self) -> io::Result<Vec<Item>> {
        let mut dirs = vec![];
        let mut files = vec![];

        for item in std::fs::read_dir(&self.cwd)?.flatten() {
            let item_type = item.file_type()?;
            let item_name = item
                .file_name()
                .to_str()
                .ok_or(io::Error::other("Couldn't get filename of item."))?
                .to_string();

            if item_type.is_dir() {
                dirs.push(Item {
                    name: item_name,
                    item_type: ItemType::Directory,
                })
            } else if item_type.is_file() {
                files.push(Item {
                    name: item_name,
                    item_type: ItemType::File,
                })
            }
        }
        let mut items = dirs;
        items.append(&mut files);
        Ok(items)
    }

    fn print_line(
        &mut self,
        text: &str,
        x: u16,
        y: u16,
        color: Color,
        highlighted: bool,
    ) -> io::Result<()> {
        queue!(self.stdout, cursor::MoveTo(x, y))?;
        queue!(self.stdout, SetForegroundColor(color))?;
        if highlighted {
            queue!(self.stdout, SetBackgroundColor(Color::White))?;
            queue!(self.stdout, SetForegroundColor(Color::Black))?;
        }
        print!("{}", text);
        if highlighted {
            queue!(self.stdout, SetBackgroundColor(Color::Reset))?;
        }
        Ok(())
    }
    fn draw_text(&mut self) -> io::Result<()> {
        let dir_color = Color::Rgb {
            r: self.config.dir_color[0],
            g: self.config.dir_color[1],
            b: self.config.dir_color[2],
        };
        let file_color = Color::Rgb {
            r: self.config.file_color[0],
            g: self.config.file_color[1],
            b: self.config.file_color[2],
        };
        for index in self.scroll..get_terminal_height()? + self.scroll {
            let length = self.current_contents.len();

            if index >= length as u16 {
                continue;
            }
            let item = &self.current_contents[index as usize];
            let name = &item.name.to_owned();
            let mut color = dir_color;

            if item.is_file() {
                color = file_color;
            }
            self.print_line(name, 0, index - self.scroll, color, self.selection == index)?;
        }
        queue!(self.stdout, ResetColor)?;
        Ok(())
    }
    fn select(&mut self) -> io::Result<()> {
        for (index, item) in self.current_contents.iter().enumerate() {
            if index as u16 == self.selection {
                match item.item_type {
                    ItemType::Directory => {
                        self.cwd.push(&item.name);
                        self.selection = 0;
                        self.scroll = 0;
                        self.current_contents = self.get_cwd_contents()?;
                    }
                    ItemType::File => {
                        let mut filepath = self.cwd.clone();
                        filepath.push(&item.name);

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
                            .ok_or(io::Error::other("Couldn't convert path to str."))?;

                        for part in command {
                            if part == "$f" {
                                parts.push_back(filepath_str.to_string());
                            } else {
                                parts.push_back(part.to_string());
                            }
                        }

                        let first = parts.pop_front();
                        if let Some(executable) = first {
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
                    }
                }
                break;
            }
        }
        Ok(())
    }
    fn go_back(&mut self) -> io::Result<()> {
        let parent = self.cwd.parent();
        if let Some(parent) = parent {
            self.cwd = parent.to_path_buf();
            self.selection = 0;
            self.scroll = 0;
            self.current_contents = self.get_cwd_contents()?;
        }
        Ok(())
    }
    fn move_up(&mut self) -> io::Result<()> {
        if self.selection == 0 {
            self.selection = self.current_contents.len() as u16 - 1;
            self.scroll = cmp::max(
                0,
                self.current_contents.len() as i16 - get_terminal_height()? as i16,
            ) as u16;
        } else {
            self.selection -= 1;
            if self.scroll > self.selection {
                self.scroll -= 1;
            }
        }
        Ok(())
    }
    fn move_down(&mut self) -> io::Result<()> {
        if self.selection >= self.current_contents.len() as u16 - 1 {
            self.selection = 0;
            self.scroll = 0;
        } else {
            self.selection += 1;
            if self.selection - self.scroll >= get_terminal_height()? {
                self.scroll += 1;
            }
        }
        Ok(())
    }
    fn handle_keypress(&mut self, event: Event) -> io::Result<()> {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Up => self.move_up()?,
                    KeyCode::Down => self.move_down()?,
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

    fn listen(&mut self) -> io::Result<()> {
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

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    text_editor_command: Vec<String>,
    binary_editor_command: Vec<String>,
    wait_for_editor_exit: bool,
    dir_color: [u8; 3],
    file_color: [u8; 3],
}
impl Config {
    fn default_config() -> Self {
        Config {
            text_editor_command: vec!["nano".to_string(), "$f".to_string()],
            binary_editor_command: vec!["hexedit".to_string(), "$f".to_string()],
            wait_for_editor_exit: true,
            dir_color: [59, 120, 255],
            file_color: [46, 199, 219],
        }
    }
}

fn get_terminal_height() -> io::Result<u16> {
    Ok(crossterm::terminal::size()?.1 - 1)
}

fn is_valid_utf8(path: &PathBuf) -> io::Result<bool> {
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

fn append_to_path(p: PathBuf, s: &str) -> PathBuf {
    let mut p = p.into_os_string();
    p.push(s);
    p.into()
}

fn get_config() -> Result<Config, Box<dyn std::error::Error>> {
    let base_config_directory =
        config_dir().ok_or(Error::other("Couldn't get config directory"))?;

    if !Path::exists(&base_config_directory) {
        Err(Error::other("Base config directory doesn't exist"))?;
    }

    let config_directory = append_to_path(base_config_directory, "/fee");

    if !Path::exists(&config_directory) {
        std::fs::create_dir(&config_directory)?;
    }

    let config_file_path = append_to_path(config_directory, "/config.json");

    if Path::exists(&config_file_path) {
        return Ok(serde_json::from_str(&std::fs::read_to_string(
            &config_file_path,
        )?)?);
    }

    let default_config = Config::default_config();
    std::fs::write(&config_file_path, serde_json::to_string(&default_config)?)?;

    Ok(default_config)
}

fn main() {
    let cwd = current_dir().unwrap();
    let config = get_config().expect("Couldn't load config!");

    let mut fee = Fee::new(cwd, config);
    fee.listen().unwrap();
}
