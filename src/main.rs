use clap::{App, Arg, SubCommand};
use crossterm::{terminal, Attribute, ClearType};
use std::path::Path;

use arete::*;

mod horizontal_menu;
use horizontal_menu::{horizontal_menu_select, HorizontalMenuOption};
mod review_session;
use review_session::{ReviewSession, REVIEW_SESSION_TIME_BOX_DEFAULT_MINUTES};

fn usage(app: &mut App) {
    let mut out = std::io::stdout();
    app.write_long_help(&mut out)
        .expect("Failed to write to stdout");
    println!();
}

fn delete_command(pk: i32) {
    eprintln!(
        "Really delete exercise {}? Type 'delete' without quotes to continue",
        pk
    );
    let mut buffer = String::new();
    if std::io::stdin().read_line(&mut buffer).is_err() {
        eprintln!("Invalid response");
        return;
    }

    let trimmed_input = buffer.trim();
    if trimmed_input != "delete" {
        eprintln!(
            "Got response \"{}\" but needed \"delete\" to proceed.",
            trimmed_input
        );
        return;
    }

    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    match service.delete_by_pk(pk) {
        Ok(_) => println!("Exercise {} has been deleted.", pk),
        Err(e) => eprintln!("Error: {}", e),
    }
}

fn edit_command(pk: i32, path: &Path) {
    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    match service.unwrap().get_by_pk(pk) {
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
            let service = ExerciseService::new_live();

            if let Err(e) = service {
                eprintln!("Error starting up: {}", e);
                return;
            }

            let service = service.unwrap();

            match service.get_by_pk(updated_exercise.id) {
                Some(mut exercise) => {
                    exercise.update_with_values(&updated_exercise);
                    if let Err(e) = exercise.update(&service) {
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
            eprintln!("Error reading in file {}: {}", path.display(), e);
        }
    }
}

fn bootstrap_schema_command() {
    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    if let Err(e) = service.bootstrap_schema() {
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

    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    if let Err(e) = service.drop_schema() {
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
    if exercise.id.is_some() {
        println!("ID:\n  {}", &exercise.id.unwrap());
    }
    print_labeled_field("Source", &exercise.source);
    print_labeled_field("Reference", &exercise.reference_answer);
}

fn print_partial_exercise(exercise: &Exercise) {
    print_labeled_field("Description", &exercise.description);
    print_labeled_field("Source", &exercise.source);
}

fn grep_command(query: &str) {
    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    println!("Searching for '{}': ", &query);
    let results = service.grep(query);

    if results.is_empty() {
        println!("No results found.");
    } else {
        for result in results {
            // TODO highlighting the matches would be nice
            print_full_exercise(&result);
            println!();
        }
    }
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

            let service = ExerciseService::new_live();

            if let Err(e) = service {
                eprintln!("Error starting up: {}", e);
                return;
            }

            let service = service.unwrap();

            if !service.schema_is_loaded() {
                eprintln!("Schema is not loaded. Please run bootstrap_schema.");
                return;
            }

            if let Err(e) = service.save_parsed_exercises(&exercises) {
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

fn schedule_command() {
    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    if !service.schema_is_loaded() {
        eprintln!("Schema is not loaded. Please run bootstrap_schema.");
        return;
    }

    let schedule = service.get_schedule();

    if schedule.is_empty() {
        println!("No exercises are loaded.");
    }

    for (date, count) in schedule {
        println!("{}: {}", date, count);
    }
}

fn count_command() {
    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    if !service.schema_is_loaded() {
        eprintln!("Schema is not loaded. Please run bootstrap_schema.");
        return;
    }

    let stats = service.get_exercise_stats();

    if stats.is_none() {
        println!("No exercises are loaded.");
        return;
    }

    let (exercise_cnt, earliest_exercise) = stats.unwrap();

    println!(
        "{} exercises. Earliest exercise created {}.",
        exercise_cnt, earliest_exercise
    );

    let due_cnt = service.count_due().unwrap_or(0);

    println!("{} exercises are currently due.\n", due_cnt);
}

fn test_connection_command() {
    if let Err(e) = ExerciseService::new_live() {
        eprintln!("Error starting up live connection: {}", e);
        return;
    }
    if let Err(e) = ExerciseService::maybe_new_test() {
        eprintln!("Error starting up test connection: {}", e);
        return;
    }

    println!("Live and test connections succeeded.");
}

fn ls_command() {
    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    if !service.schema_is_loaded() {
        eprintln!("Schema is not loaded. Please run bootstrap_schema.");
        return;
    }

    let exercises = service.get_all_by_due_date_desc();

    if exercises.is_empty() {
        println!("No exercises loaded.");
        return;
    }

    println!("{} exercises:\n", exercises.len());

    // TODO page these the way git log does
    for exercise in exercises.iter() {
        print_full_exercise(&exercise);
        println!("Due at:\n  {}\n", &exercise.due_at);
    }
}

fn due_command() {
    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    if !service.schema_is_loaded() {
        eprintln!("Schema is not loaded. Please run bootstrap_schema.");
        return;
    }

    let exercises = service.get_due();

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

fn confirm_exercise_answer(exercise: &mut Exercise, service: &ExerciseService) {
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
                if let Err(e) = exercise.update(&service) {
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
    if horizontal_menu_select(&[HorizontalMenuOption::new("Continue", 'c')]).is_err() {
        std::process::exit(1);
    }
}

fn clear_screen() {
    let terminal = terminal();
    terminal.clear(ClearType::All).unwrap();
}

fn review_command(time_box_minutes: Option<i64>) {
    let service = ExerciseService::new_live();

    if let Err(e) = service {
        eprintln!("Error starting up: {}", e);
        return;
    }

    let service = service.unwrap();

    if !service.schema_is_loaded() {
        eprintln!("Schema is not loaded. Please run bootstrap_schema.");
        return;
    }

    let mut exercises = service.get_due();

    if exercises.is_empty() {
        println!("No exercises are due.");
        return;
    }

    let exercise_cnt = exercises.len();

    clear_screen();

    let review_session = ReviewSession::new(time_box_minutes);

    for (i, exercise) in exercises.iter_mut().enumerate() {
        // we could set a timer that prints this as soon as time elapses, but
        // waiting until the next exercise is finished to end it seems fine
        if review_session.has_exceeded_timebox() {
            clear_screen();
            println!("Whoops! The allotted review time of {} minutes has elapsed. Not all exercises were completed ({} remain).", review_session.time_box_minutes(), exercise_cnt - i);
            println!("But, not to worry! Take a break and do the rest later or finish tomorrow. What matters is that you keep trying and keep working on building strong habits.");
            return;
        }

        println!(
            "{}{}{}\n",
            Attribute::Bold,
            review_session.exercise_display_str(i, exercise_cnt, &exercise),
            Attribute::Reset
        );

        println!("{}\n", &exercise.description);

        let options = [
            HorizontalMenuOption::new("Know it", 'y'),
            HorizontalMenuOption::new("Don't know it", 'n'),
            HorizontalMenuOption::new("Quit and edit", 'e'),
        ];

        match horizontal_menu_select(&options) {
            Ok(result) => match result {
                Some(selected_index) => {
                    if selected_index == 0 {
                        confirm_exercise_answer(exercise, &service);
                    } else if selected_index == 1 {
                        print!("\n\n");
                        print_labeled_field("Reference", &exercise.reference_answer);
                        print_labeled_field("Source", &exercise.source);

                        exercise.update_repetition_interval(false);
                        if let Err(e) = exercise.update(&service) {
                            eprintln!("\n\nError saving exercise: {}", e);
                        }
                    } else {
                        // quit and edit
                        if !&exercise.id.is_some() {
                            eprintln!("\n\nExercise has no ID, can't export!");
                            std::process::exit(1);
                        }

                        let output_name = format!("edited_exercise_{}.yaml", &exercise.id.unwrap());
                        let output_path = Path::new(&output_name[..]);
                        match &exercise.yaml_export(output_path) {
                            Ok(_) => {
                                println!(
                                    "\n\nExported exercise to file '{}' for editing. Exiting.",
                                    output_path.display()
                                );
                                std::process::exit(0);
                            }
                            Err(e) => {
                                eprintln!("\n\nError exporting exercise: {}", e);
                                std::process::exit(1);
                            }
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
    println!(
        "\n{} exercises reviewed in {} minutes.",
        exercise_cnt,
        review_session.elapsed_minutes().num_minutes()
    );
}

fn main() {
    let review_str = format!(
        "Review due exercises. Limited by default to {} minutes",
        REVIEW_SESSION_TIME_BOX_DEFAULT_MINUTES
    );

    let mut app = App::new("arete")
        .version("0.1.0")
        .author("Warren Henning <warren.henning@gmail.com>")
        .about("Simple command-line flashcard application")
        .subcommand(
            SubCommand::with_name("bootstrap_schema")
                .about("Bootstrap the database schema. Run this first."),
        )
        .subcommand(
            SubCommand::with_name("drop_schema")
                .about("Drop the database schema. Normally not needed."),
        )
        .subcommand(
            SubCommand::with_name("import").about("Import a file").arg(
                Arg::with_name("path")
                    .index(1)
                    .help("The file to import.")
                    .required(true),
            ),
        )
        .subcommand(
            SubCommand::with_name("check")
                .about("Checks if an input YAML is valid.")
                .arg(
                    Arg::with_name("path")
                        .help("The file to check.")
                        .index(1)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("edit")
                .about("Export an exercise for later import.")
                .arg(
                    Arg::with_name("id")
                        .help("Primary key of the exercise to export.")
                        .index(1)
                        .required(true),
                )
                .arg(
                    Arg::with_name("output_path")
                        .help("Path to write the file to")
                        .index(2)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("update")
                .about("Update an existing exercise in place.")
                .arg(
                    Arg::with_name("path")
                        .help("The file with an exercise to update.")
                        .index(1)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("grep")
                .about("Search for exercises containing a string.")
                .arg(
                    Arg::with_name("query")
                        .help("The string to search for (including the ID field).")
                        .index(1)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("delete")
                .about("Delete an exercise by ID.")
                .arg(
                    Arg::with_name("ID")
                        .help("Primary key of the exercise to delete.")
                        .index(1)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("test_connection")
                .about("Test the database configuration in config.toml."),
        )
        .subcommand(SubCommand::with_name("count").about("Count exercises."))
        .subcommand(SubCommand::with_name("ls").about("List all exercuses by due date descending."))
        .subcommand(
            SubCommand::with_name("due").about("List all due exercises by due date descending."),
        )
        .subcommand(
            SubCommand::with_name("schedule").about("List dates when exercises will be due"),
        )
        .subcommand(
            SubCommand::with_name("review").about(&review_str[..]).arg(
                Arg::with_name("minutes")
                    .help("Number of minutes to spend reviewing")
                    .takes_value(true)
                    .index(1),
            ),
        );

    let matches = app.clone().get_matches();

    if matches.subcommand_name().is_none() {
        usage(&mut app);
        return;
    }

    // arguments in subcommands don't work properly. yes, I checked the docs and
    // GitHub. matches.value_of("<param>") is None for all of them and
    // matches.is_present() returns false, so it won't work, so I have to do
    // this, so no, I am not misusing the library any more than is apparently
    // necessary. let me know if I've done something wrong and there is a way to
    // make this work that actually does work.
    let args = std::env::args().collect::<Vec<_>>();

    match matches.subcommand_name().unwrap() {
        "bootstrap_schema" => {
            bootstrap_schema_command();
            return;
        }
        "drop_schema" => {
            drop_schema_command();
            return;
        }
        "import" => {
            // see comment above
            let path = &args[2];
            import_command(&path, false);
            return;
        }
        "check" => {
            // see comment above
            let path = &args[2];
            import_command(&path, true);
            return;
        }
        "edit" => {
            // see comment above
            let id_str = &args[2];
            let output_path = &args[3];
            match id_str.parse::<i32>() {
                Ok(id) => edit_command(id, Path::new(&output_path)),
                Err(_) => eprintln!("Cannot convert '{}' to a primary key", id_str),
            }
            return;
        }
        "update" => {
            // see comment above
            let path = &args[2];
            update_exercise_from_path(Path::new(&path));
            return;
        }
        "grep" => {
            // see comment above
            let query = &args[2];
            grep_command(&query);
            return;
        }
        "delete" => {
            // see comment above
            let id_str = &args[2];
            match id_str.parse::<i32>() {
                Ok(id) => delete_command(id),
                Err(_) => eprintln!("Cannot convert '{}' to a primary key", id_str),
            }
            return;
        }
        "count" => {
            count_command();
            return;
        }
        "test_connection" => {
            test_connection_command();
            return;
        }
        "ls" => {
            ls_command();
            return;
        }
        "due" => {
            due_command();
            return;
        }
        "schedule" => {
            schedule_command();
            return;
        }
        "review" => {
            // see comment above
            if args.len() == 3 {
                let minutes_str = &args[2];
                match minutes_str.parse::<i64>() {
                    Ok(minutes) => review_command(Some(minutes)),
                    Err(_) => eprintln!("Cannot convert '{}' to a minute amount", minutes_str),
                }
            } else {
                review_command(None);
            }
            return;
        }
        _ => {}
    }

    usage(&mut app);
}
