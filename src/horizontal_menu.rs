use std::path::Path;
use crossterm::{cursor, input, Attribute, InputEvent, KeyEvent, RawScreen, TerminalCursor};
use std::io;
use std::io::Write;

pub struct HorizontalMenuOption<'a> {
    pub label: &'a str,
    pub shortcut: char,
}

impl<'a> HorizontalMenuOption<'a> {
    pub fn new(label: &'a str, shortcut: char) -> HorizontalMenuOption<'a> {
        HorizontalMenuOption { label, shortcut }
    }
}

// quick and dirty debugging mechanism for debugging TUI apps. run touch debug.log && tail -f debug.log in a separate window,
// then debug statements will show up there without messing up your UI.
#[allow(dead_code)]
pub fn debug_log_print(msg: String) {
    use std::fs::OpenOptions;

    let mut file = {
        if Path::new("debug.log").exists() {
            OpenOptions::new().append(true).open("debug.log").unwrap()
        } else {
            OpenOptions::new().create(true).open("debug.log").unwrap()
        }
    };

    file.write_all(msg.as_bytes()).unwrap();
    file.flush().unwrap();
}

pub fn draw_horizontal_menu(
    options: &[HorizontalMenuOption],
    cursor: &TerminalCursor,
    selected_index: usize,
    max_selected_index: usize,
) -> io::Result<()> {
    // NOTE(warren): we shouldn't need to clear the line since we're just going to overwrite it anyway

    // TODO maybe make this configurable?
    let option_separator = {
        if cfg!(windows) {
            "|"
        } else {
            "│"
        }
    };

    // go to beginning of current line
    let (_, current_line) = cursor.pos();
    cursor.goto(0, current_line)?;

    for (i, option) in options.iter().enumerate() {
        // print the current state of the menu
        if i == selected_index {
            print!(
                "{}{} ({}){}",
                Attribute::Reverse,
                option.label,
                option.shortcut,
                Attribute::Reset
            );
        } else {
            print!("{} ({})", option.label, option.shortcut);
        }

        if i < max_selected_index {
            print!(" {} ", option_separator);
        }

        io::stdout().flush()?;
    }
    Ok(())
}

// draws a selectable horizontal menu which you can use arrow keys, h/l (a la vim), Ctrl-b/Ctrl-f (a la Emacs), or Ctrl-a/Ctrl-e (a la Emacs),
// and Esc/Ctrl to exit.
pub fn horizontal_menu_select(options: &[HorizontalMenuOption]) -> io::Result<Option<usize>> {
    // TODO maybe handle mouse events to make options clickable?

    let mut did_select = false;
    let mut done = false;
    let mut selected_index = 0;
    let max_selected_index = options.len() - 1;

    let cursor = cursor();

    // makes for a slightly nicer interface
    cursor.hide()?;

    while !done {
        draw_horizontal_menu(&options, &cursor, selected_index, max_selected_index)?;

        // drop into raw mode for input handling
        let _screen = RawScreen::into_raw_mode()?;

        let input = input();
        let mut sync_reader = input.read_sync();

        // read input until a valid key (h/l/left/right/Esc) is entered. disregard other input
        loop {
            if let Some(key_event) = sync_reader.next() {
                if let InputEvent::Keyboard(key_press) = key_event {
                    match key_press {
                        KeyEvent::Ctrl('a') => {
                            selected_index = 0;
                            break;
                        }
                        KeyEvent::Ctrl('e') => {
                            selected_index = max_selected_index;
                            break;
                        }
                        KeyEvent::Char('h') | KeyEvent::Left | KeyEvent::Up | KeyEvent::Ctrl('b') => {
                            if selected_index >= 1 {
                                selected_index -= 1;
                                break;
                            }
                        }
                        KeyEvent::Char('l') | KeyEvent::Right | KeyEvent::Down | KeyEvent::Ctrl('f') => {
                            if selected_index < max_selected_index {
                                selected_index += 1;
                                break;
                            }
                        }
                        KeyEvent::Char('\n') => {
                            did_select = true;
                            done = true;
                            break;
                        }
                        KeyEvent::Esc | KeyEvent::Ctrl(_) => {
                            did_select = false;
                            done = true;
                            break;
                        }
                        KeyEvent::Char(c) => {
                            // see if the key stroke matches any of the shortcuts
                            let mut found_match = false;
                            for (i, option) in options.iter().enumerate() {
                                if c == option.shortcut {
                                    selected_index = i;
                                    did_select = true;
                                    done = true;
                                    found_match = true;
                                    // redraw to show result of selection
                                    draw_horizontal_menu(
                                        &options,
                                        &cursor,
                                        selected_index,
                                        max_selected_index,
                                    )?;
                                    break; // this for loop only
                                }
                            }
                            if found_match {
                                break; // loop { above
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // the screen will drop here, putting us back into canonical mode when we re-print the menu
    }

    // want to reshow the cursor since the cursor hide would otherwise persist even after exiting
    cursor.show()?;

    if did_select {
        Ok(Some(selected_index))
    } else {
        Ok(None)
    }
}
