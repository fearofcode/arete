#[macro_use]
extern crate serde_derive;
use chrono::{Duration, Local, NaiveDate};
use postgres::rows::Row;
use postgres::types::ToSql;
use postgres::{Connection, TlsMode};
use postgres::transaction::Transaction;
use std::error::Error;
use std::path::Path;
use crossterm::{cursor, input, terminal, ClearType, Attribute, InputEvent, KeyEvent, RawScreen, TerminalCursor};
use std::io;
use std::io::Write;


#[derive(Deserialize)]
struct DbConfig {
    live_url: String,
    #[allow(dead_code)]
    test_url: String,
}

fn todays_date() -> NaiveDate {
    Local::today().naive_local()
}

fn read_config_file() -> Result<DbConfig, Box<Error>> {
    let config_str = std::fs::read_to_string("config.toml")?;

    match toml::from_str(&config_str) {
        Ok(toml) => {
            let config: DbConfig = toml;
            Ok(config)
        }
        Err(e) => Err(Box::new(e)),
    }
}

fn connect_to_main_database(config: &DbConfig) -> postgres::Result<Connection> {
    Connection::connect(&config.live_url[..], TlsMode::None)
}

fn bootstrap_schema(conn: &Connection) -> postgres::Result<u64> {
    conn.execute(
        "create table if not exists exercises(
        id serial primary key,
        created_at date not null default current_date,
        description text unique not null,
        source text not null,
        reference_answer text not null,
        due_at date not null default current_date,
        update_interval integer not null default 0,
        consecutive_successful_reviews integer not null default 0
    )",
        &[],
    )?;

    conn.execute(
        "create index if not exists exercises_due_at on exercises(due_at)",
        &[],
    )
}

fn drop_schema(conn: &Connection) -> postgres::Result<u64> {
    conn.execute("drop table if exists exercises cascade", &[])
}

fn schema_is_loaded(conn: &Connection) -> bool {
    let query = "SELECT EXISTS (
        SELECT 1
        FROM   information_schema.tables 
        WHERE  table_schema = 'public'
        AND    table_name = 'exercises'
    )";

    match &conn.query(query, &[]).unwrap().iter().next() {
        Some(row) => {
            let result: bool = row.get(0);
            result
        }
        None => false,
    }
}

#[derive(Debug)]
struct Exercise {
    id: Option<i32>,
    created_at: NaiveDate,
    due_at: NaiveDate,
    description: String,
    source: String,
    reference_answer: String,
    update_interval: i32,
    consecutive_successful_reviews: i32,
}

impl PartialEq<Exercise> for Exercise {
    fn eq(&self, other: &Exercise) -> bool {
        self.id == other.id
    }
}

const ONE_DAY: i32 = 1;
const MAX_INTERVAL: i32 = ONE_DAY * 90;
/* keep this fixed for now */
const EASINESS_FACTOR: i32 = 2;

impl Exercise {
    fn new(description: &str, source: &str, reference_answer: &str) -> Exercise {
        let today = todays_date();
        Exercise {
            id: None,
            created_at: today,
            due_at: today,
            description: String::from(description),
            source: String::from(source),
            reference_answer: String::from(reference_answer),
            update_interval: 0,
            consecutive_successful_reviews: 0,
        }
    }

    fn new_from_row(row: &Row) -> Exercise {
        Exercise {
            id: Some(row.get(0)),
            created_at: row.get(1),
            due_at: row.get(2),
            description: row.get(3),
            source: row.get(4),
            reference_answer: row.get(5),
            update_interval: row.get(6),
            consecutive_successful_reviews: row.get(6),
        }
    }

    fn get_due(conn: &Connection) -> Vec<Exercise> {
        let mut exercises = vec![];

        let due_query = "
        SELECT 
            id, created_at, due_at, description, source, reference_answer, update_interval, consecutive_successful_reviews
        FROM
            exercises
        WHERE
            due_at <= $1
        ORDER BY
            due_at desc, 
            id desc";
    
        let today = todays_date();

        for row in &conn
            .query(&due_query, &[&today])
            .unwrap()
        {
            exercises.push(Exercise::new_from_row(&row));
        }

        exercises
    }
    fn get_all_by_due_date_desc(conn: &Connection) -> Vec<Exercise> {
        let mut exercises = vec![];

        let due_query = "
        SELECT 
            id, created_at, due_at, description, source, reference_answer, update_interval, consecutive_successful_reviews
        FROM
            exercises
        ORDER BY
            due_at desc, 
            id desc";

        for row in &conn
            .query(&due_query, &[])
            .unwrap()
        {
            exercises.push(Exercise::new_from_row(&row));
        }

        exercises
    }

    fn create(&self, tx: &Transaction) -> Result<u64, Box<dyn Error>> {
        // exercise was already inserted
        if self.id.is_some() {
            return Err(make_error("Cannot insert, has PK".to_string()))
        }

        // we can let postgres insert some defaults
        let values: &[&ToSql] = &[
            &self.created_at,
            &self.due_at,
            &self.description,
            &self.source,
            &self.reference_answer,
        ];

        // the code doesn't really need the generated values when creating, so I don't feel the need to write the code to fill in data
        // for fields I don't actually need
        let query = "insert into exercises(created_at, due_at, description, source, reference_answer) values($1, $2, $3, $4, $5)";
        match tx.execute(query, values) {
            Ok(i) => Ok(i),
            Err(e) => Err(Box::new(e))
        }
    }

    fn update(&mut self, conn: &Connection) -> Result<u64, Box<dyn Error>> {
        if self.id.is_none() {
            return Err(make_error("Cannot insert, has no PK".to_string()))
        }

        let query = "update exercises set created_at = $1, due_at = $2, description = $3, source = $4, 
        reference_answer = $5, update_interval = $6, consecutive_successful_reviews = $7 where id = $8";

        let values: &[&ToSql] = &[
            &self.created_at,
            &self.due_at,
            &self.description,
            &self.source,
            &self.reference_answer,
            &self.update_interval,
            &self.consecutive_successful_reviews,
            &self.id.unwrap(),
        ];
        match conn.execute(query, &values) {
            Ok(i) => Ok(i),
            Err(e) => Err(Box::new(e))
        }
    }

    fn update_repetition_interval(&mut self, correct: bool) {
        self.due_at = todays_date();

        if correct {
            self.consecutive_successful_reviews += 1;
            self.update_interval = match self.consecutive_successful_reviews {
                1 => ONE_DAY,
                _ => std::cmp::min(MAX_INTERVAL, self.update_interval * EASINESS_FACTOR),
            };

            self.due_at = self.due_at + Duration::days(self.update_interval as i64);
        } else {
            self.consecutive_successful_reviews = 0;
            self.update_interval = 0;
        }
    }
}

fn make_error(error_string: String) -> Box<Error> {
    Box::new(std::io::Error::new(std::io::ErrorKind::Other, error_string))
}

#[derive(PartialEq)]
enum FileParsingState {
    Beginning,
    ReadingDescription,
    ReadingSource,
    ReadingReferenceAnswer,
}

fn parse_exercises(path: &Path) -> Result<Vec<Exercise>, Box<Error>> {
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return Err(Box::new(e)),
    };
    let mut exercises: Vec<Exercise> = vec![];

    let description_tag = "description:";
    let description_tag_length = description_tag.len();
    let source_tag = "source:";
    let source_tag_length = source_tag.len();
    let reference_answer_tag = "reference_answer:";
    let reference_answer_tag_length = reference_answer_tag.len();

    let mut state = FileParsingState::Beginning;

    exercises.push(Exercise::new("", "", ""));

    let mut current_exercise = &mut exercises[0];
    let mut current_exercise_index = 0;

    for (line_index, line) in content.lines().enumerate() {
        let human_line = line_index + 1;
        let trimmed_line = line.trim();
        let is_line_empty = line.is_empty();

        let mut is_field_continuation = (line.len() >= 3) &&
            line.chars().nth(0).unwrap_or('x').is_whitespace() &&
            line.chars().nth(1).unwrap_or('x').is_whitespace();
        if is_field_continuation {
            let mut contains_non_whitespace = false;
            for ch in line.chars().skip(2) {
                if !ch.is_whitespace() {
                    contains_non_whitespace = true;
                    break;
                }
            }
            is_field_continuation = contains_non_whitespace;
        }

        match state {
            FileParsingState::Beginning => {
                if trimmed_line.starts_with(description_tag) {
                    state = FileParsingState::ReadingDescription;
                    current_exercise.description =
                        trimmed_line[description_tag_length..].trim().to_string();
                    if current_exercise.description.is_empty() {
                        let error_msg = format!(
                            "Description on line {} is blank.", human_line
                        );

                        return Err(make_error(error_msg))
                    }
                } else if !line.is_empty() {
                    let error_msg = format!(
                        "Expected line {} (\"{}\") to either be a description or a blank line.",
                        human_line, line
                    );

                    return Err(make_error(error_msg))
                }
            }
            FileParsingState::ReadingDescription => {
                if trimmed_line.starts_with(source_tag) {
                    state = FileParsingState::ReadingSource;
                    current_exercise.source = trimmed_line[source_tag_length..].trim().to_string();
                    if current_exercise.source.is_empty() {
                        let error_msg = format!(
                            "Source on line {} is blank.", human_line
                        );

                        return Err(make_error(error_msg))
                    }
                } else if is_field_continuation {
                    current_exercise.description += &("\n".to_string() + trimmed_line);
                } else if !is_line_empty {
                    let error_msg = format!(
                        "Expected to keep reading a description or find a source on line {}.",
                        human_line
                    );

                    return Err(make_error(error_msg))
                }
                current_exercise.description = current_exercise.description.trim().to_string();
            },
            FileParsingState::ReadingSource => {
                if trimmed_line.starts_with(reference_answer_tag) {
                    state = FileParsingState::ReadingReferenceAnswer;
                    current_exercise.reference_answer = trimmed_line[reference_answer_tag_length..].trim().to_string();
                    if current_exercise.reference_answer.is_empty() {
                        let error_msg = format!(
                            "Reference answer on line {} is blank.", human_line
                        );

                        return Err(make_error(error_msg))
                    }
                } else if is_field_continuation {
                    current_exercise.source += &("\n".to_string() + trimmed_line);
                } else if !is_line_empty {
                    let error_msg = format!(
                        "Expected to keep reading a source or find a reference answer on line {}.",
                        human_line
                    );

                    return Err(make_error(error_msg))
                }
                current_exercise.source = current_exercise.source.trim().to_string();
            },  
            FileParsingState::ReadingReferenceAnswer => {
                if trimmed_line.starts_with(description_tag) {
                    current_exercise.reference_answer = current_exercise.reference_answer.trim().to_string();

                    if current_exercise.description.is_empty() || current_exercise.source.is_empty() || current_exercise.reference_answer.is_empty() {
                        let error_msg = format!(
                            "Exercise {} does not have all its fields filled out.",
                            current_exercise_index + 1
                        );

                        return Err(make_error(error_msg))
                    }
                    // new exercise
                    state = FileParsingState::ReadingDescription;

                    exercises.push(Exercise::new("", "", ""));
                    current_exercise_index += 1;
                    current_exercise = &mut exercises[current_exercise_index];
                    current_exercise.description = trimmed_line[description_tag_length..].trim().to_string();
                } else if is_field_continuation {
                    current_exercise.reference_answer += &("\n".to_string() + trimmed_line);
                    current_exercise.reference_answer = current_exercise.reference_answer.trim().to_string();
                }
            }
        }
    }

    if state != FileParsingState::ReadingReferenceAnswer {
        Err(make_error("Expected file to end with a reference answer.".to_string()))
    } else {
        Ok(exercises)
    }
}

fn save_parsed_exercises(exercises: &Vec<Exercise>, conn: &Connection) -> Result<(), Box<dyn Error>> {
    // @Robustness I don't know how to do the types to return whatever type that could be generated by the query
    let tx = conn.transaction().unwrap();

    for exercise in exercises {
        // @Performance we could probably do bulk inserts but for small files it won't matter
        if let Err(e) = exercise.create(&tx) {
            // rollback will kick in
            return Err(e)
        } 
    }
    match tx.commit() {
        Ok(_) => Ok(()),
        Err(e) => Err(Box::new(e))
    }
}

fn usage(app_name: &String) {
    println!("Usage: {} <command> [command-param]\n", app_name);
    println!("Available commands:\n");
    println!("  bootstrap_schema\tBootstrap the database schema");
    println!("  drop_schema\t\tDrop the database schema");
    println!("  import <path>\t\tImport a file");
    println!("  ls\t\t\tList all exercises by due date descending");
    println!("  review\t\tReview due exercises");
}

fn bootstrap_live_database_connection() -> Result<Connection, Box<dyn Error>> {
    let config = read_config_file()?;
    let conn = connect_to_main_database(&config)?;

    Ok(conn)
}

fn bootstrap_schema_command() {
    let conn = bootstrap_live_database_connection();

    if let Err(e) = conn {
        eprintln!("Error starting up: {}", e);
        return;
    }

    if let Err(e) = bootstrap_schema(&conn.unwrap()) {
        eprintln!("Error bootstrapping database: {}", e);
        return;
    }

    println!("Database schema bootstrapped.");
}

fn drop_schema_command() {
    eprintln!("This will irreversibly drop all data in the database! Are you sure you want to proceed? Type 'drop schema' without quotes to proceed.");
    let mut buffer = String::new();
    if let Err(_) = std::io::stdin().read_line(&mut buffer) {
        eprintln!("Invalid response");
        return;
    }

    let trimmed_input = buffer.trim();
    if trimmed_input != "drop schema" {
        eprintln!("Got response \"{}\" but needed \"drop schema\" to proceed.", trimmed_input);
        return;
    }

    let conn = bootstrap_live_database_connection();

    if let Err(e) = conn {
        eprintln!("Error starting up: {}", e);
        return;
    }

    if let Err(e) = drop_schema(&conn.unwrap()) {
        eprintln!("Error dropping database: {}", e);
        return;
    }

    println!("Database schema dropped.");
}

const EXCERPT_LENGTH: usize = 80;

fn string_excerpt(s: &String) -> &str {
    if s.len() <= EXCERPT_LENGTH {
        return &s[..]
    }
    return &s[..EXCERPT_LENGTH] 
}

fn import_command(path: &String) {
    match parse_exercises(Path::new(path)) {
        Ok(exercises) => {
            let conn = bootstrap_live_database_connection();

            if let Err(e) = conn {
                eprintln!("Error starting up: {}", e);
                return;
            }

            let conn = conn.unwrap();

            if !schema_is_loaded(&conn) {
                eprintln!("Schema is not loaded. Please run bootstrap_schema.");
                return;
            }
            println!("Here are the exercises that are about to be imported: ");

            for exercise in exercises.iter() {
                println!("Description: {}", string_excerpt(&exercise.description));
                println!("Source: {}", string_excerpt(&exercise.source));
                println!("Reference: {}\n", string_excerpt(&exercise.reference_answer));
            }

            println!("Import all of these? [y/N]");
            let mut buffer = String::new();
            if let Err(_) = std::io::stdin().read_line(&mut buffer) {
                eprintln!("Invalid response");
                return;
            }

            let trimmed_input = buffer.trim();
            if trimmed_input != "y" {
                eprintln!("Got response \"{}\" but needed \"y\" to proceed. No data was saved.", trimmed_input);
                return;
            }
            
            if let Err(e) = save_parsed_exercises(&exercises, &conn) {
                eprintln!("Error saving exercises: {}", e);
                eprintln!("The most likely cause of this is a duplicate description.");
                return;
            }

            println!("Imported {} exercises.", exercises.len());
        },
        Err(e) => {
            eprintln!("Error parsing {}: {}", path, e);
        }
    }
}

fn ls_command() {
    let conn = bootstrap_live_database_connection();

    if let Err(e) = conn {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let conn = conn.unwrap();

    if !schema_is_loaded(&conn) {
        eprintln!("Schema is not loaded. Please run bootstrap_schema.");
        return;
    }

    let exercises = Exercise::get_all_by_due_date_desc(&conn);

    if exercises.is_empty() {
        println!("No exercises loaded.");
        return;
    }

    println!("{} exercises:\n", exercises.len());

    // TODO page these the way git log does
    for exercise in exercises.iter() {
        println!("Description: {}", string_excerpt(&exercise.description));
        println!("Source: {}", string_excerpt(&exercise.source));
        println!("Due at: {}\n", &exercise.due_at);
    }
}

struct HorizontalMenuOption<'a> {
    label: &'a str,
    shortcut: char,
}

impl<'a> HorizontalMenuOption<'a> {
    fn new(label: &'a str, shortcut: char) -> HorizontalMenuOption<'a> {
        HorizontalMenuOption { label, shortcut }
    }
}

// quick and dirty debugging mechanism for debugging TUI apps. run touch debug.log && tail -f debug.log in a separate window,
// then debug statements will show up there without messing up your UI.
#[allow(dead_code)]
fn debug_log_print(msg: String) {
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

fn draw_horizontal_menu(
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
fn horizontal_menu_select(options: &[HorizontalMenuOption]) -> io::Result<Option<usize>> {
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

fn confirm_exercise_answer(exercise: &mut Exercise, conn: &Connection) {
    println!("\n\nSource: {}", &exercise.source);
    println!("Answer: {}\n", &exercise.reference_answer);

    println!("Is the answer you had in mind correct?");

    let confirmation_options = [
        HorizontalMenuOption::new("Yes", 'y'),
        HorizontalMenuOption::new("No", 'n'),
    ];

    loop {
        match horizontal_menu_select(&confirmation_options) {
            Ok(result) => match result {
                Some(selected_index) => {
                    let was_correct = selected_index == 0;
                    exercise.update_repetition_interval(was_correct);
                    if let Err(e) = exercise.update(&conn) {
                        eprintln!("\nError saving exercise: {}", e);
                    }

                    if was_correct {
                        println!("\n\nMarked exercise correct.\n");
                    } else {
                        println!("\n\nMarked exercise incorrect.\n");
                    }
                    break;
                }
                None => {
                    eprintln!("\nNo selection was made.");
                    std::process::exit(1);
                }
            },
            _ => {
                eprintln!("\nI/O error while selecting option");
                std::process::exit(1);
            }
        }

    }
}

fn print_next_exercise_input() {
    loop {
        if let Ok(_) = horizontal_menu_select(&vec![HorizontalMenuOption::new("Next exercise", 'n')]) {
            break;
        } else {
            std::process::exit(1);
        }
    }
}

fn clear_screen() {
    let terminal = terminal();
    terminal.clear(ClearType::All).unwrap();
}

fn review_command() {
    let conn = bootstrap_live_database_connection();

    if let Err(e) = conn {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let conn = conn.unwrap();

    if !schema_is_loaded(&conn) {
        eprintln!("Schema is not loaded. Please run bootstrap_schema.");
        return;
    }

    let mut exercises = Exercise::get_due(&conn);

    if exercises.is_empty() {
        println!("No exercises are due.");
        return;
    }

    let exercise_cnt = exercises.len();

    clear_screen();

    for (i, exercise) in exercises.iter_mut().enumerate() {
        println!("{}Exercise {}/{}{}\n", Attribute::Bold, i + 1, exercise_cnt, Attribute::Reset);

        println!("{}\n", &exercise.description);
        
        let options = [
            HorizontalMenuOption::new("Know it", 'y'),
            HorizontalMenuOption::new("Don't know it", 'n'),
        ];

        loop {
            match horizontal_menu_select(&options) {
                Ok(result) => match result {
                    Some(selected_index) => {
                        if selected_index == 0 {
                            confirm_exercise_answer(exercise, &conn);
                        } else {
                            println!("\n\nSource: {}\n", &exercise.source);
                            println!("Answer: {}\n", &exercise.reference_answer);

                            exercise.update_repetition_interval(false);
                            if let Err(e) = exercise.update(&conn) {
                                eprintln!("Error saving exercise: {}", e);
                            }
                        }
                        print_next_exercise_input();
                        break;
                    }
                    None => {
                        eprintln!("\nNo selection was made.");
                        std::process::exit(1);
                    }
                },
                _ => {
                    eprintln!("\nI/O error while selecting option");
                    std::process::exit(1);
                }
            }

        }

        // clear the screen if not last exercise
        if i < exercise_cnt - 1 {
            clear_screen();
        }
    }

    println!("\n\n{}Done reviewing!{}", Attribute::Bold, Attribute::Reset);
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();

    let app_name = &args[0];
 
    match args.len() {
        2 => {
            let command = &args[1];
            
            match &command[..] {
                "bootstrap_schema" => bootstrap_schema_command(),
                "drop_schema" => drop_schema_command(),
                "ls" => ls_command(),
                "review" => review_command(),
                _ => {
                    eprintln!("Unknown command '{}'", &command);
                    usage(app_name);
                }
            }
        },
        3 => {
            let command = &args[1];

            let param = &args[2];

            match &command[..] {
                "import" => import_command(&param),
                _ => {
                    eprintln!("Unknown command '{}'", &command);
                    usage(app_name);
                }
            }
        }
        _ => usage(app_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stringify_boxed_error(e: Box<Error>) -> String {
        format!("{}", e)
    }

    fn boostrap_test_database() -> Connection {
        // it's OK to unwrap here because we're in a test environment and failing here is fine
        let config = read_config_file().unwrap();

        let conn = Connection::connect(config.test_url, postgres::TlsMode::None).unwrap();

        /* handle failed test runs that didn't clean up properly */
        drop_schema(&conn).unwrap();
        bootstrap_schema(&conn).unwrap();

        conn
    }

    #[test]
    fn test_schema_bootstrapping_dropping_loaded() {
        let conn = boostrap_test_database();

        assert!(schema_is_loaded(&conn));

        drop_schema(&conn).unwrap();

        assert!(!schema_is_loaded(&conn));

        bootstrap_schema(&conn).unwrap();
        assert!(schema_is_loaded(&conn));

        drop_schema(&conn).unwrap();

        assert!(!schema_is_loaded(&conn));
    }

    #[test]
    fn test_valid_multiline_exercise() {
        let exercises = parse_exercises(
            &Path::new("sample_files")
                .join("valid")
                .join("multi_line_inputs.txt"),
        )
        .unwrap();

        assert!(exercises.len() == 2);
        assert!(exercises[0].description == "foo\nmore foo\none more, should be trimmed.");
        assert!(exercises[0].source == "here is a single-line source");
        assert!(exercises[0].reference_answer == "here is some more content\na tab in here");

        assert!(exercises[1].description == "single-line here");
        assert!(exercises[1].source == "this is multiple lines\nsee, multiple lines");
        assert!(exercises[1].reference_answer == "this is single-line, too");
    }

    #[test]
    fn test_parsing_error_handling() {
        // test each of the sample files

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("completely_invalid.txt")
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                assert_eq!(stringify_boxed_error(e), "Expected line 1 (\"blah\") to either be a description or a blank line.");
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("missing_reference_answer.txt")
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                assert_eq!(stringify_boxed_error(e), "Expected file to end with a reference answer.");
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("second_missing_source.txt")
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                assert_eq!(stringify_boxed_error(e), "Expected file to end with a reference answer.");
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("missing_field.txt")
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(err_string, "Expected to keep reading a description or find a source on line 2.");
            } else {
                assert!(false);
            }
        }
        
        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("only_tag.txt")
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(err_string, "Description on line 1 is blank.");
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("blank_source.txt")
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(err_string, "Source on line 2 is blank.");
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("blank_source.txt")
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(err_string, "Source on line 2 is blank.");
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("blank_reference_answer.txt")
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(err_string, "Reference answer on line 4 is blank.");
            } else {
                assert!(false);
            }
        }
    }

    #[test]
    fn test_save_parsed_exercises() {
        let exercises = vec![
            Exercise::new("foo", "bar", "baz"),
            Exercise::new("foo 2", "bar 2", "baz 2"),
        ];

        let conn = boostrap_test_database();

        save_parsed_exercises(&exercises, &conn).unwrap();

        let mut saved_exercises = Exercise::get_all_by_due_date_desc(&conn);

        assert_eq!(saved_exercises.len(), 2);

        let today = todays_date();

        assert_eq!(saved_exercises[0].id, Some(2));
        assert_eq!(saved_exercises[0].created_at, today);
        assert_eq!(saved_exercises[0].due_at, today);
        assert_eq!(saved_exercises[0].description, "foo 2");
        assert_eq!(saved_exercises[0].source, "bar 2");
        assert_eq!(saved_exercises[0].reference_answer, "baz 2");
        assert_eq!(saved_exercises[0].update_interval, 0);
        assert_eq!(saved_exercises[0].consecutive_successful_reviews, 0);

        assert_eq!(saved_exercises[1].id, Some(1));
        assert_eq!(saved_exercises[1].created_at, today);
        assert_eq!(saved_exercises[1].due_at, today);
        assert_eq!(saved_exercises[1].description, "foo");
        assert_eq!(saved_exercises[1].source, "bar");
        assert_eq!(saved_exercises[1].reference_answer, "baz");
        assert_eq!(saved_exercises[1].update_interval, 0);
        assert_eq!(saved_exercises[1].consecutive_successful_reviews, 0);

        let first_exercise = &mut saved_exercises[0];
        first_exercise.due_at += Duration::days(1);
        assert!(first_exercise.update(&conn).is_ok());

        let due = Exercise::get_due(&conn);

        assert_eq!(due[0], saved_exercises[1]);
    }

    #[test]
    fn test_save_parsed_exercise_transaction_handling() {
        let exercises = vec![
            Exercise::new("foo", "bar", "baz"),
            // duplicate description
            Exercise::new("foo", "bar 2", "baz 2"),
        ];

        let conn = boostrap_test_database();
        let result = save_parsed_exercises(&exercises, &conn);

        assert!(result.is_err());

        let e = result.unwrap_err();

        let error_string = stringify_boxed_error(e);

        assert_eq!(error_string, "database error: ERROR: duplicate key value violates unique constraint \"exercises_description_key\"");
    }

    // test interval updating
    #[test]
    fn test_exercise_update_interval_calculations() {
        let mut exercise = Exercise::new("", "", "");

        let today = Local::today().naive_local();

        assert_eq!(exercise.due_at, today);
        assert_eq!(exercise.consecutive_successful_reviews, 0);
        assert_eq!(exercise.update_interval, 0);

        exercise.update_repetition_interval(true);
        assert_eq!(exercise.due_at, today + Duration::days(1));
        assert_eq!(exercise.consecutive_successful_reviews, 1);
        assert_eq!(exercise.update_interval, 1);

        exercise.update_repetition_interval(true);
        assert_eq!(exercise.due_at, today + Duration::days(2));
        assert_eq!(exercise.consecutive_successful_reviews, 2);
        assert_eq!(exercise.update_interval, 2);

        exercise.update_repetition_interval(true);
        assert_eq!(exercise.due_at, today + Duration::days(4));
        assert_eq!(exercise.consecutive_successful_reviews, 3);
        assert_eq!(exercise.update_interval, 4);

        exercise.update_repetition_interval(true);
        assert_eq!(exercise.due_at, today + Duration::days(8));
        assert_eq!(exercise.consecutive_successful_reviews, 4);
        assert_eq!(exercise.update_interval, 8);

        exercise.update_repetition_interval(true);
        assert_eq!(exercise.due_at, today + Duration::days(16));
        assert_eq!(exercise.consecutive_successful_reviews, 5);
        assert_eq!(exercise.update_interval, 16);

        exercise.update_repetition_interval(true);
        assert_eq!(exercise.due_at, today + Duration::days(32));
        assert_eq!(exercise.consecutive_successful_reviews, 6);
        assert_eq!(exercise.update_interval, 32);

        exercise.update_repetition_interval(true);
        assert_eq!(exercise.due_at, today + Duration::days(64));
        assert_eq!(exercise.consecutive_successful_reviews, 7);
        assert_eq!(exercise.update_interval, 64);

        for i in 1..100 {
            exercise.update_repetition_interval(true);
            assert_eq!(exercise.due_at, today + Duration::days(90));
            assert_eq!(exercise.consecutive_successful_reviews, 7 + i);
            assert_eq!(exercise.update_interval, 90);
        }

        exercise.update_repetition_interval(false);
        assert_eq!(exercise.due_at, today);
        assert_eq!(exercise.consecutive_successful_reviews, 0);
        assert_eq!(exercise.update_interval, 0);

        exercise.update_repetition_interval(true);
        assert_eq!(exercise.due_at, today + Duration::days(1));
        assert_eq!(exercise.consecutive_successful_reviews, 1);
        assert_eq!(exercise.update_interval, 1);
    }

    // test a simulated review update process end to end (update an exercise's fields, check that they get saved in database)
    #[test]
    fn test_review_crud_update_process() {
        let exercise = Exercise::new("foo", "bar", "baz");

        let conn = boostrap_test_database();
        save_parsed_exercises(&vec![exercise], &conn).unwrap();

        let mut saved_exercises = Exercise::get_all_by_due_date_desc(&conn);

        assert_eq!(saved_exercises.len(), 1);

        let saved_exercise = &mut saved_exercises[0];

        let today = todays_date();

        assert_eq!(saved_exercise.id, Some(1));
        assert_eq!(saved_exercise.created_at, today);
        assert_eq!(saved_exercise.due_at, today);
        assert_eq!(saved_exercise.description, "foo");
        assert_eq!(saved_exercise.source, "bar");
        assert_eq!(saved_exercise.reference_answer, "baz");
        assert_eq!(saved_exercise.update_interval, 0);
        assert_eq!(saved_exercise.consecutive_successful_reviews, 0);

        saved_exercise.update_repetition_interval(true);

        assert_eq!(saved_exercise.id, Some(1));
        assert_eq!(saved_exercise.created_at, today);
        assert_eq!(saved_exercise.due_at, today + Duration::days(1));
        assert_eq!(saved_exercise.description, "foo");
        assert_eq!(saved_exercise.source, "bar");
        assert_eq!(saved_exercise.reference_answer, "baz");
        assert_eq!(saved_exercise.update_interval, 1);
        assert_eq!(saved_exercise.consecutive_successful_reviews, 1);

        saved_exercise.update(&conn).unwrap();

        let saved_exercises = Exercise::get_all_by_due_date_desc(&conn);

        assert_eq!(saved_exercises.len(), 1);

        let saved_exercise = &saved_exercises[0];

        assert_eq!(saved_exercise.id, Some(1));
        assert_eq!(saved_exercise.created_at, today);
        assert_eq!(saved_exercise.due_at, today + Duration::days(1));
        assert_eq!(saved_exercise.description, "foo");
        assert_eq!(saved_exercise.source, "bar");
        assert_eq!(saved_exercise.reference_answer, "baz");
        assert_eq!(saved_exercise.update_interval, 1);
        assert_eq!(saved_exercise.consecutive_successful_reviews, 1);
    }

}
