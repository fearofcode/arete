extern crate serde_derive;
use crossterm::{terminal, Attribute, ClearType};
use postgres::Connection;
use std::error::Error;
use std::path::Path;

use arete::*;

mod horizontal_menu;
use horizontal_menu::{horizontal_menu_select, HorizontalMenuOption};

fn usage(app_name: &str) {
    println!("Usage: {} <command> [command_param]\n", app_name);
    println!("Available commands:\n");
    println!("  bootstrap_schema\t\tBootstrap the database schema. Run this first.");
    println!("  drop_schema\t\t\tDrop the database schema. Normally not needed.");
    println!("  import <path> [--dry_run|-d]\tImport a file.");
    println!(
        "  check <path>\t\t\tChecks if an input YAML is valid. Equivalent to import --dry_run."
    );
    // shitty kludge feature
    println!("  edit <id> <output_path>\tExport an existing exercise for later import. Placeholder feature until I implement an editor here.");
    println!("  update <path>\t\t\tUpdate an existing exercise in place.");
    println!("  count\t\t\t\tCount exercises.");
    println!("  ls\t\t\t\tList all exercises by due date descending.");
    println!("  due\t\t\t\tList all due exercises by due date descending.");
    println!("  review\t\t\tReview due exercises. The main thing this application is meant to do.");
}

fn bootstrap_live_database_connection() -> Result<Connection, Box<dyn Error>> {
    let config = read_config_file()?;
    let conn = connect_to_main_database(&config)?;

    Ok(conn)
}

fn edit_command(pk: i32, path: &Path) {
    let conn = bootstrap_live_database_connection();

    if let Err(e) = conn {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let conn = conn.unwrap();

    match Exercise::get_by_pk(pk, &conn) {
        Some(exercise) => {
            if let Err(e) = exercise.yaml_export(&path) {
                eprintln!("Error while exporting: {}", e);
            }
        }
        None => {
            eprintln!("Couldn't find exercise with ID {}.", pk);
        }
    }
}

fn update_exercise_from_path(path: &Path) {
    match parse_updated_exercise(&path) {
        Ok(updated_exercise) => {
            let conn = bootstrap_live_database_connection();

            if let Err(e) = conn {
                eprintln!("Error starting up: {}", e);
                return;
            }

            let conn = conn.unwrap();

            match Exercise::get_by_pk(updated_exercise.id, &conn) {
                Some(mut exercise) => {
                    exercise.update_with_values(&updated_exercise);
                    if let Err(e) = exercise.update(&conn) {
                        eprintln!("Error saving updated exercise: {}", e);
                        return;
                    }

                    println!("Exercise {} has been updated.", &exercise.id.unwrap());
                }
                None => {
                    eprintln!("Exercise with ID {} does not exist", updated_exercise.id);
                }
            }
        }
        Err(e) => {
            eprintln!("Error reading in file: {}", e);
        }
    }
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
    if std::io::stdin().read_line(&mut buffer).is_err() {
        eprintln!("Invalid response");
        return;
    }

    let trimmed_input = buffer.trim();
    if trimmed_input != "drop schema" {
        eprintln!(
            "Got response \"{}\" but needed \"drop schema\" to proceed.",
            trimmed_input
        );
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

fn print_labeled_field(label: &str, s: &str) {
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

fn print_partial_exercise(exercise: &Exercise) {
    print_labeled_field("Description", &exercise.description);
    print_labeled_field("Source", &exercise.source);
}

fn import_command(path: &str, dry_run: bool) {
    match parse_exercises(Path::new(path)) {
        Ok(exercises) => {
            if dry_run {
                println!("Here are the exercises that would be imported:\n");
            } else {
                println!("Here are the exercises that are about to be imported:\n");
            }

            for exercise in exercises.iter() {
                print_full_exercise(exercise);
                println!();
            }

            if dry_run {
                println!("\nExiting since this is a dry run.");
                return;
            }
            println!("Import all of these? [y/N]");
            let mut buffer = String::new();
            if std::io::stdin().read_line(&mut buffer).is_err() {
                eprintln!("Invalid response");
                return;
            }

            let trimmed_input = buffer.trim();
            if trimmed_input != "y" {
                eprintln!(
                    "Got response \"{}\" but needed \"y\" to proceed. No data was saved.",
                    trimmed_input
                );
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
        }
        Err(e) => {
            eprintln!("Error parsing {}: {}", path, e);
        }
    }
}

fn count_command() {
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

    println!("{} exercises.", exercises.len());

    let due = Exercise::get_due(&conn);

    println!("{} exercises are currently due.\n", due.len());
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
        print_full_exercise(&exercise);
        if exercise.id.is_some() {
            println!("ID:\n  {}", &exercise.id.unwrap());
        } else {
            println!("ID:\n  ???? No ID, this is a bug");
        }
        println!("Due at:\n  {}\n", &exercise.due_at);
    }
}

fn due_command() {
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

    let exercises = Exercise::get_due(&conn);

    if exercises.is_empty() {
        println!("No exercises are currently due. Run 'ls' to see exercises due later.");
        return;
    }

    println!("{} exercises:\n", exercises.len());

    // TODO page these the way git log does
    for exercise in exercises.iter() {
        print_partial_exercise(&exercise);
        if exercise.id.is_some() {
            println!("ID:\n  {}", &exercise.id.unwrap());
        } else {
            println!("ID:\n  ???? No ID, this is a bug");
        }
        println!("Due at:\n  {}\n", &exercise.due_at);
    }
}

fn confirm_exercise_answer(exercise: &mut Exercise, conn: &Connection) {
    print!("\n\n");
    print_labeled_field("Reference", &exercise.reference_answer);
    print_labeled_field("Source", &exercise.source);

    println!("Is the answer you had in mind correct?");

    let confirmation_options = [
        HorizontalMenuOption::new("Yes", 'y'),
        HorizontalMenuOption::new("No", 'n'),
    ];

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

fn print_next_exercise_input() {
    if horizontal_menu_select(&[HorizontalMenuOption::new("Next exercise", 'n')]).is_err() {
        std::process::exit(1);
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
        println!(
            "{}Exercise {}/{} - ID {}{}\n",
            Attribute::Bold,
            i + 1,
            exercise_cnt,
            &exercise.id.unwrap_or(-1),
            Attribute::Reset
        );

        println!("{}\n", &exercise.description);

        let options = [
            HorizontalMenuOption::new("Know it", 'y'),
            HorizontalMenuOption::new("Don't know it", 'n'),
        ];

        match horizontal_menu_select(&options) {
            Ok(result) => match result {
                Some(selected_index) => {
                    if selected_index == 0 {
                        confirm_exercise_answer(exercise, &conn);
                    } else {
                        print!("\n\n");
                        print_labeled_field("Reference", &exercise.reference_answer);
                        print_labeled_field("Source", &exercise.source);

                        exercise.update_repetition_interval(false);
                        if let Err(e) = exercise.update(&conn) {
                            eprintln!("Error saving exercise: {}", e);
                        }
                    }
                    print_next_exercise_input();
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
                "due" => due_command(),
                "count" => count_command(),
                "review" => review_command(),
                _ => {
                    if command != "--help" && command != "-h" && command != "help" {
                        eprintln!("Unknown command '{}'", &command);
                    }
                    usage(app_name);
                }
            }
        }
        3 => {
            let command = &args[1];

            let param = &args[2];

            match &command[..] {
                "import" => import_command(&param, false),
                // check is a synonym for import --dry_run
                "check" => import_command(&param, true),
                "update" => update_exercise_from_path(Path::new(&param)),
                _ => {
                    eprintln!("Unknown command '{}'", &command);
                    usage(app_name);
                }
            }
        }
        4 => {
            let command = &args[1];
            let param = &args[2];
            let command_option = &args[3];

            if command == "import" && (command_option == "--dry_run" || command_option == "-d") {
                import_command(&param, true);
            } else if command == "edit" {
                match param.parse::<i32>() {
                    Ok(pk) => edit_command(pk, Path::new(command_option)),
                    Err(_) => eprintln!("Cannot convert '{}' to a primary key", param),
                }
            } else {
                usage(app_name);
            }
        }
        _ => usage(app_name),
    }
}
