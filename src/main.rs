extern crate serde_derive;
use postgres::{Connection};
use std::error::Error;
use std::path::Path;
use crossterm::{terminal, ClearType, Attribute};

use arete::*;

mod horizontal_menu;
use horizontal_menu::{HorizontalMenuOption, horizontal_menu_select};

fn usage(app_name: &String) {
    println!("Usage: {} <command> [command_param]\n", app_name);
    println!("Available commands:\n");
    println!("  bootstrap_schema\tBootstrap the database schema");
    println!("  drop_schema\t\tDrop the database schema");
    println!("  import <path> [--dry_run|-d]\t\tImport a file");
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

fn print_labeled_field(label: &str, s: &String) {
    println!("{}:", label);
    for line in s.lines() {
        println!("  {}", line);
    }
}

fn print_full_exercise(exercise: &Exercise) {
    print_labeled_field("Description", &exercise.description);
    print_labeled_field("Source", &exercise.source);
    print_labeled_field("Reference", &exercise.reference_answer);
}

fn import_command(path: &String, dry_run: bool) {
    match parse_exercises(Path::new(path)) {
        Ok(exercises) => {
            if dry_run {
                println!("Here are the exercises that would be imported:\n");
            } else {
                println!("Here are the exercises that are about to be imported:\n");
            }

            for exercise in exercises.iter() {
                print_full_exercise(exercise);
            }

            if dry_run {
                println!("\nExiting since this is a dry run.");
                return;
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
            
            /* No need to connect to the database unless actually necessary */

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
        print_labeled_field("Description", &exercise.description);
        print_labeled_field("Source", &exercise.source);
        println!("Due at:\n  {}\n", &exercise.due_at);
    }
}

fn confirm_exercise_answer(exercise: &mut Exercise, conn: &Connection) {
    print!("\n\n");
    print_labeled_field("Source", &exercise.source);
    print_labeled_field("Reference", &exercise.reference_answer);

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
                            print!("\n\n");
                            print_labeled_field("Source", &exercise.source);
                            print_labeled_field("Reference", &exercise.reference_answer);

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
                "import" => import_command(&param, false),
                _ => {
                    eprintln!("Unknown command '{}'", &command);
                    usage(app_name);
                }
            }
        },
        4 => {
            let command = &args[1];
            let param = &args[2];
            let command_option = &args[3];

            if command == "import" && (command_option == "--dry_run" || command_option == "-d") {
                import_command(&param, true);
            } else {
                usage(app_name);
            }
        },
        _ => usage(app_name)
    }
}
