use chrono::{Duration, Local, NaiveDate};
use postgres::rows::Row;
use postgres::transaction::Transaction;
use postgres::types::ToSql;
use postgres::{Connection, TlsMode};
use serde_derive::Deserialize;
use std::error::Error;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
pub struct DbConfig {
    live_url: String,
    #[allow(dead_code)]
    test_url: String,
}

pub fn read_config_file() -> Result<DbConfig, Box<Error>> {
    let config_str = std::fs::read_to_string("config.toml")?;

    match toml::from_str(&config_str) {
        Ok(toml) => {
            let config: DbConfig = toml;
            Ok(config)
        }
        Err(e) => Err(Box::new(e)),
    }
}

pub fn connect_to_main_database(config: &DbConfig) -> postgres::Result<Connection> {
    Connection::connect(&config.live_url[..], TlsMode::None)
}

pub fn bootstrap_schema(conn: &Connection) -> postgres::Result<u64> {
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

pub fn drop_schema(conn: &Connection) -> postgres::Result<u64> {
    conn.execute("drop table if exists exercises cascade", &[])
}

pub fn schema_is_loaded(conn: &Connection) -> bool {
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
fn todays_date() -> NaiveDate {
    Local::today().naive_local()
}

#[derive(Debug)]
pub struct Exercise {
    pub id: Option<i32>,
    pub created_at: NaiveDate,
    pub due_at: NaiveDate,
    pub description: String,
    pub source: String,
    pub reference_answer: String,
    pub update_interval: i32,
    pub consecutive_successful_reviews: i32,
}

impl PartialEq<Exercise> for Exercise {
    fn eq(&self, other: &Exercise) -> bool {
        self.id == other.id
    }
}

#[derive(Debug, Deserialize)]
pub struct ExportedExercise {
    pub id: i32,
    pub description: String,
    pub source: String,
    pub reference_answer: String,
}

pub const ONE_DAY: i32 = 1;
pub const MAX_INTERVAL: i32 = ONE_DAY * 90;
/* keep this fixed for now */
pub const EASINESS_FACTOR: i32 = 2;

fn pad_multiline_string(s: &str) -> String {
    s.lines()
        .map(|line| "  ".to_owned() + line)
        .collect::<Vec<_>>()
        .join("\n")
}

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

    pub fn update_with_values(&mut self, updated_exercise: &ExportedExercise) {
        self.description = updated_exercise.description.clone();
        self.source = updated_exercise.source.clone();
        self.reference_answer = updated_exercise.reference_answer.clone();
    }

    pub fn yaml_export(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        if self.id.is_none() {
            return Err(make_error(
                "Cannot export an exercise that has not been saved".to_string(),
            ));
        }
        let exported_exercise = ExportedExercise {
            id: self.id.unwrap(),
            description: self.description.clone(),
            source: self.source.clone(),
            reference_answer: self.reference_answer.clone(),
        };

        // we could use serde_yaml for this, but it won't print newlines nicely.
        // since our data model is pretty simple, we can get away with just
        // constructing the string ourselves.

        let yaml_string = format!(
            "---
id: {}
description: |+
{}
source: |+
{}
reference_answer: |+
{}
",
            exported_exercise.id,
            pad_multiline_string(&exported_exercise.description),
            pad_multiline_string(&exported_exercise.source),
            pad_multiline_string(&exported_exercise.reference_answer)
        );

        match fs::write(path, yaml_string) {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(e)),
        }
    }

    fn sql_column_list() -> &'static str {
        "id, created_at, due_at, description, source, reference_answer, update_interval, 
        consecutive_successful_reviews"
    }

    pub fn get_by_pk(pk: i32, conn: &Connection) -> Option<Exercise> {
        let query = format!(
            "
        SELECT 
            {}
        FROM
            exercises
        WHERE
            id = $1
        ",
            Exercise::sql_column_list()
        );

        for row in &conn.query(&query, &[&pk]).unwrap() {
            return Some(Exercise::new_from_row(&row));
        }

        None
    }

    pub fn get_due(conn: &Connection) -> Vec<Exercise> {
        let mut exercises = vec![];

        let due_query = format!(
            "
        SELECT 
            {}
        FROM
            exercises
        WHERE
            due_at <= $1
        ORDER BY
            due_at desc, 
            id desc",
            Exercise::sql_column_list()
        );

        let today = todays_date();

        for row in &conn.query(&due_query, &[&today]).unwrap() {
            exercises.push(Exercise::new_from_row(&row));
        }

        exercises
    }

    pub fn grep(conn: &Connection, query_string: &str) -> Vec<Exercise> {
        let mut exercises = vec![];

        let grep_query = format!(
            "
        SELECT 
            {}
        FROM
            exercises
        WHERE
            description like ('%' || $1 || '%')
            or source like ('%' || $1 || '%')
            or reference_answer like ('%' || $1 || '%')
            or id::text like ('%' || $1 || '%')
        ORDER BY
            due_at desc, 
            id desc",
            Exercise::sql_column_list()
        );

        for row in &conn.query(&grep_query, &[&query_string]).unwrap() {
            exercises.push(Exercise::new_from_row(&row));
        }

        exercises
    }

    pub fn get_all_by_due_date_desc(conn: &Connection) -> Vec<Exercise> {
        let mut exercises = vec![];

        let due_query = format!(
            "
        SELECT 
            {}
        FROM
            exercises
        ORDER BY
            due_at desc, 
            id desc",
            Exercise::sql_column_list()
        );

        for row in &conn.query(&due_query, &[]).unwrap() {
            exercises.push(Exercise::new_from_row(&row));
        }

        exercises
    }

    fn create(&self, tx: &Transaction) -> Result<u64, Box<dyn Error>> {
        // exercise was already inserted
        if self.id.is_some() {
            return Err(make_error("Cannot insert, has PK".to_string()));
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
            Err(e) => Err(Box::new(e)),
        }
    }

    pub fn update(&mut self, conn: &Connection) -> Result<u64, Box<dyn Error>> {
        if self.id.is_none() {
            return Err(make_error("Cannot insert, has no PK".to_string()));
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
            Err(e) => Err(Box::new(e)),
        }
    }

    pub fn update_repetition_interval(&mut self, correct: bool) {
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

#[derive(Debug, Deserialize)]
struct ImportedExercise {
    pub description: String,
    pub source: String,
    pub reference_answer: String,
}

fn convert_yaml_str_to_exercises(s: &str) -> Result<Vec<ImportedExercise>, serde_yaml::Error> {
    serde_yaml::from_str(s)
}

fn convert_yaml_str_to_updated_exercise(s: &str) -> Result<ExportedExercise, serde_yaml::Error> {
    serde_yaml::from_str(s)
}

fn yaml_string_is_empty(s: &String) -> bool {
    s.trim().is_empty() || s == "~"
}

pub fn parse_exercises(path: &Path) -> Result<Vec<Exercise>, Box<dyn Error>> {
    let content = std::fs::read_to_string(&path)?;

    match convert_yaml_str_to_exercises(&content) {
        Ok(exercises) => {
            for (i, exercise) in exercises.iter().enumerate() {
                let human_index = i + 1;
                if yaml_string_is_empty(&exercise.description) {
                    return Err(make_error(format!(
                        "Exercise {} has a blank or missing description.",
                        human_index
                    )));
                } else if yaml_string_is_empty(&exercise.source) {
                    return Err(make_error(format!(
                        "Exercise {} has a blank or missing source.",
                        human_index
                    )));
                } else if yaml_string_is_empty(&exercise.reference_answer) {
                    return Err(make_error(format!(
                        "Exercise {} has a blank or missing reference answer.",
                        human_index
                    )));
                }
            }
            Ok(exercises
                .iter()
                .map(|e| {
                    Exercise::new(
                        &e.description.trim(),
                        &e.source.trim(),
                        &e.reference_answer.trim(),
                    )
                })
                .collect::<Vec<_>>())
        }
        Err(yaml_err) => Err(Box::new(yaml_err)),
    }
}

pub fn parse_updated_exercise(path: &Path) -> Result<ExportedExercise, Box<dyn Error>> {
    let content = std::fs::read_to_string(&path)?;

    match convert_yaml_str_to_updated_exercise(&content) {
        Ok(mut exercise) => {
            if yaml_string_is_empty(&exercise.description) {
                return Err(make_error(
                    "Exercise has a blank or missing description.".to_string(),
                ));
            } else if yaml_string_is_empty(&exercise.source) {
                return Err(make_error(
                    "Exercise has a blank or missing source.".to_string(),
                ));
            } else if yaml_string_is_empty(&exercise.reference_answer) {
                return Err(make_error(
                    "Exercise has a blank or missing reference answer.".to_string(),
                ));
            }
            exercise.description = exercise.description.trim().to_string();
            exercise.source = exercise.source.trim().to_string();
            exercise.reference_answer = exercise.reference_answer.trim().to_string();
            Ok(exercise)
        }
        Err(yaml_err) => Err(Box::new(yaml_err)),
    }
}

pub fn save_parsed_exercises(
    exercises: &Vec<Exercise>,
    conn: &Connection,
) -> Result<(), Box<dyn Error>> {
    let tx = conn.transaction()?;

    for exercise in exercises {
        // @Performance we could probably do bulk inserts but for small files it won't matter
        if let Err(e) = exercise.create(&tx) {
            // rollback will kick in
            return Err(e);
        }
    }
    match tx.commit() {
        Ok(_) => Ok(()),
        Err(e) => Err(Box::new(e)),
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
                .join("multi_line_inputs.yaml"),
        )
        .unwrap();

        assert_eq!(exercises.len(), 2);
        assert_eq!(
            exercises[0].description,
            "foo\nmore foo\none more, should be trimmed."
        );
        assert_eq!(exercises[0].source, "here is a single-line source");
        assert_eq!(
            exercises[0].reference_answer,
            "here is some more content\na tab in here"
        );

        assert_eq!(exercises[1].description, "single-line here");
        assert_eq!(
            exercises[1].source,
            "this is multiple lines\nsee, multiple lines"
        );
        assert_eq!(exercises[1].reference_answer, "this is single-line, too");
    }

    #[test]
    fn test_indentation_preserved() {
        let exercises = parse_exercises(
            &Path::new("sample_files")
                .join("valid")
                .join("indentation_preserved.yaml"),
        )
        .unwrap();

        assert_eq!(exercises.len(), 1);
        assert_eq!(
            exercises[0].description,
            "Write out psuedocode for the Fisher-Yates shuffle."
        );
        assert_eq!(
            exercises[0].source,
            "Wikipedia (https://en.wikipedia.org/wiki/Fisher%E2%80%93Yates_shuffle)"
        );
        assert_eq!(exercises[0].reference_answer, "for i from 0 to n-2 do\n  j <- random integer such that i <= j < n\n  exchange a[i] and a[j]");
    }
    #[test]
    fn test_valid_longer_example() {
        let exercises = parse_exercises(
            &Path::new("sample_files")
                .join("valid")
                .join("thinking_like_a_programmer.yaml"),
        )
        .unwrap();

        assert_eq!(exercises.len(), 1);
        assert_eq!(exercises[0].description, "A farmer with a fox, a goose, and a sack of corn needs to cross a river.\nThe farmer has a rowboat, but there is room for only the farmer and one\nof his three items. Unfortunately, both the fox and the goose are hungry.\nThe fox cannot be left alone with the goose, or the fox will eat the\ngoose. Likewise, the goose cannot be left alone with the sack of corn, or\nthe goose will eat the corn. How does the farmer get everything across\nthe river?");
        assert_eq!(exercises[0].source, "Thinking Like A Programmer, p. 3");
        assert_eq!(exercises[0].reference_answer, "The key is to take things back after moving them. First take the goose\nacross, leaving the fox with the corn: (f c, g). Take the fox across.\nTake the goose back to the corn: (g c, f). Now we\'re home free: take the\ncorn across and leave it with the fox. Go back, get the corn, and bring\nit across.\n\nThe solution here is to swap the fox and the goose once the goose has\nbeen transferred.");
    }

    #[test]
    fn test_parsing_error_handling() {
        // test each of the sample files

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("completely_invalid.yaml"),
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                assert_eq!(
                    stringify_boxed_error(e),
                    "invalid type: string \"blah\", expected a sequence at line 1 column 1"
                );
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("missing_reference_answer.yaml"),
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                assert_eq!(
                    stringify_boxed_error(e),
                    ".[0]: missing field `reference_answer` at line 2 column 14"
                );
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("second_missing_source.yaml"),
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                assert_eq!(
                    stringify_boxed_error(e),
                    ".[1]: missing field `source` at line 6 column 14"
                );
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("missing_field.yaml"),
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(
                    err_string,
                    ".[0]: missing field `source` at line 2 column 14"
                );
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("only_tag.yaml"),
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(err_string, "Exercise 1 has a blank or missing description.");
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("blank_source.yaml"),
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(err_string, "Exercise 1 has a blank or missing source.");
            } else {
                assert!(false);
            }
        }

        {
            let exercises = parse_exercises(
                &Path::new("sample_files")
                    .join("invalid")
                    .join("blank_reference_answer.yaml"),
            );
            assert!(exercises.is_err());

            if let Err(e) = exercises {
                let err_string = stringify_boxed_error(e);
                assert_eq!(
                    err_string,
                    "Exercise 1 has a blank or missing reference answer."
                );
            } else {
                assert!(false);
            }
        }
    }

    #[test]
    fn test_pad_multiline_string() {
        let multiline_string = "here is a line
here is another
and one more";

        let expected = "  here is a line
  here is another
  and one more"
            .to_string();

        assert_eq!(pad_multiline_string(&multiline_string), expected);
    }

    #[test]
    fn test_grepping_for_exercises() {
        let exercises = vec![Exercise::new("foo", "bar", "baz some data here")];

        let conn = boostrap_test_database();

        save_parsed_exercises(&exercises, &conn).expect("Saving failed");

        let saved_exercises = Exercise::get_all_by_due_date_desc(&conn);

        assert_eq!(saved_exercises.len(), 1);

        let saved_exercise = &saved_exercises[0];

        assert_eq!(saved_exercise.id.expect("expected ID"), 1);

        let search_by_description = Exercise::grep(&conn, "foo");
        assert_eq!(search_by_description.len(), 1);
        assert_eq!(&search_by_description[0], saved_exercise);

        let search_by_source = Exercise::grep(&conn, "bar");
        assert_eq!(search_by_source.len(), 1);
        assert_eq!(&search_by_source[0], saved_exercise);

        let search_by_reference_answer = Exercise::grep(&conn, "some data");
        assert_eq!(search_by_reference_answer.len(), 1);
        assert_eq!(&search_by_reference_answer[0], saved_exercise);

        let search_by_id = Exercise::grep(&conn, "1");
        assert_eq!(search_by_id.len(), 1);
        assert_eq!(&search_by_id[0], saved_exercise);

        let invalid_query = Exercise::grep(&conn, "blah");
        assert_eq!(invalid_query.len(), 0);
    }

    #[test]
    fn test_export_saved_exercise() {
        let exercises = vec![Exercise::new("foo", "bar", "baz")];

        let conn = boostrap_test_database();

        save_parsed_exercises(&exercises, &conn).expect("Saving failed");

        let mut saved_exercises = Exercise::get_all_by_due_date_desc(&conn);

        assert_eq!(saved_exercises.len(), 1);

        let saved_exercise = &mut saved_exercises[0];

        let path = Path::new("id_export_test.yaml");

        if path.exists() {
            std::fs::remove_file(&path).expect("We tried to delete a file that didn't exist?");
        }
        saved_exercise.yaml_export(&path).expect("Failed to export");

        let data =
            fs::read_to_string(Path::new("id_export_test.yaml")).expect("Failed to read back in");

        println!("here is data:");
        println!("{}", data);
        assert!(data.contains("description: |+\n  foo"));
        assert!(data.contains("source: |+\n  bar"));
        assert!(data.contains("id: 1"));
        assert!(data.contains("reference_answer: |+\n  baz"));

        // test that it overwrites existing files

        saved_exercise.description = "quux".to_string();
        saved_exercise.source = "quux 2".to_string();
        saved_exercise.reference_answer = "quux 3".to_string();
        saved_exercise.yaml_export(&path).expect("Failed to export");

        let data = fs::read_to_string(&path).expect("Failed to read back in");

        assert!(data.contains("description: |+\n  quux"));
        assert!(data.contains("source: |+\n  quux 2"));
        assert!(data.contains("id: 1"));
        assert!(data.contains("reference_answer: |+\n  quux 3"));

        // test that it imports correctly

        let parsed_exercise = parse_updated_exercise(&path).expect("should not error out");

        saved_exercise.update_with_values(&parsed_exercise);
        saved_exercise.update(&conn).expect("update failed");

        let saved_exercises = Exercise::get_all_by_due_date_desc(&conn);

        assert_eq!(saved_exercises.len(), 1);

        assert_eq!(&saved_exercises[0].description, "quux");

        if path.exists() {
            std::fs::remove_file(&path).expect("We tried to delete a file that didn't exist?");
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

        assert_eq!(Exercise::get_by_pk(2, &conn).unwrap(), saved_exercises[0]);

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
