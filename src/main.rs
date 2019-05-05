#[macro_use]
extern crate serde_derive;
use chrono::{Duration, Local, NaiveDate};
use postgres::rows::Row;
use postgres::types::ToSql;
use postgres::{Connection, TlsMode};
use std::error::Error;
use std::path::Path;

#[derive(Deserialize)]
struct DbConfig {
    live_url: String,
    test_url: String,
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

fn connect_to_main_database(config: &DbConfig) -> Connection {
    Connection::connect(&config.live_url[..], TlsMode::None).unwrap()
}

fn boostrap_test_database() -> Connection {
    let config = read_config_file().unwrap();

    let conn = Connection::connect(config.test_url, postgres::TlsMode::None).unwrap();

    /* handle failed test runs that didn't clean up properly */
    drop_schema(&conn);
    bootstrap_schema(&conn);

    conn
}

fn bootstrap_schema(conn: &Connection) {
    conn.execute(
        "create table if not exists exercises(
        id serial primary key,
        created_at date not null default current_date,
        description text not null,
        source text not null,
        reference_answer text not null,
        due_at date not null default current_date,
        update_interval integer not null default 1,
        consecutive_successful_reviews integer not null default 0
    )",
        &[],
    )
    .unwrap();

    conn.execute(
        "create index if not exists exercises_due_at on exercises(due_at)",
        &[],
    )
    .unwrap();
}

fn drop_schema(conn: &Connection) {
    conn.execute("drop table if exists exercises cascade", &[])
        .unwrap();
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

const ONE_DAY: i32 = 1;
const MAX_INTERVAL: i32 = ONE_DAY * 90;
/* keep this fixed for now */
const EASINESS_FACTOR: i32 = 2;

impl Exercise {
    fn new(description: &str, source: &str, reference_answer: &str) -> Exercise {
        let today = Local::today().naive_local();
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

    fn update(&mut self, conn: &Connection) {
        if self.id.is_none() {
            panic!("Cannot update without an ID")
        }

        let query = "update exercises set created_at = $1, due_at = $2, description = $3, source = $4, 
        reference_answer = $5, update_interval = $6, consecutive_successful_reviews = $7 where id = $7";

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
        conn.execute(query, &values).unwrap();
    }

    fn sql_column_list() -> String {
        String::from("id, created_at, due_at, description, source, reference_answer, update_interval, consecutive_successful_reviews")
    }

    fn get_due(conn: &Connection) -> Vec<Exercise> {
        let mut exercises = vec![];

        /* no this is not vulnerable to fucking SQL injection, I trust my own fucking input */
        let due_query = format!(
            "
        SELECT 
            {}
        FROM
            exercises
        WHERE
            due_at <= $1
        ORDER BY
            due_at",
            Exercise::sql_column_list()
        );

        for row in &conn
            .query(&due_query, &[&Local::today().naive_local()])
            .unwrap()
        {
            exercises.push(Exercise::new_from_row(&row));
        }

        exercises
    }

    fn get_count(conn: &Connection) -> i32 {
        let due_query = "SELECT (count(*)::integer) FROM exercises";

        match &conn.query(due_query, &[]).unwrap().iter().next() {
            Some(row) => {
                let cnt: i32 = row.get(0);
                cnt
            }
            None => 0,
        }
    }

    fn update_repetition_interval(&mut self, correct: bool) {
        self.due_at = Local::today().naive_local();

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

fn main() {
    println!("Hello, world!");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stringify_boxed_error(e: Box<Error>) -> String {
        format!("{}", e)
    }

    #[test]
    fn test_schema_bootstrapping_dropping_loaded() {
        let conn = boostrap_test_database();

        assert!(schema_is_loaded(&conn));

        drop_schema(&conn);

        assert!(!schema_is_loaded(&conn));

        bootstrap_schema(&conn);
        assert!(schema_is_loaded(&conn));

        drop_schema(&conn);

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

    // test saving parsed exercises

    // test checking for duplicate exercises

    // test interval updating

    // test a simulated review update process end to end

    // test function for descriptions of all exercises (first ~100 chars, at least) and due date sorted by descending due date

    // OK, now go write main implementation code: usage, bootstrap command, drop command, import command (preview before import), list all, reviewing!
}
