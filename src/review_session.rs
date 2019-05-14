use chrono::{DateTime, Duration, Utc};
use std::fmt;

use crate::Exercise;

pub const REVIEW_SESSION_TIME_BOX_DEFAULT_MINUTES: i64 = 20;
const STANDARD_REVIEW_OUTPUT_WIDTH: usize = 80;

pub struct ReviewSession {
    start: DateTime<Utc>,
    time_box: Duration,
}

impl ReviewSession {
    pub fn time_box_default() -> Duration {
        Duration::minutes(20)
    }

    pub fn new(time_box_minutes: Option<i64>) -> ReviewSession {
        ReviewSession {
            start: Utc::now(),
            time_box: time_box_minutes
                .map_or_else(ReviewSession::time_box_default, Duration::minutes),
        }
    }

    pub fn time_box_minutes(&self) -> i64 {
        self.time_box.num_minutes()
    }

    pub fn elapsed_minutes(&self) -> Duration {
        Utc::now() - self.start
    }

    pub fn has_exceeded_timebox(&self) -> bool {
        // have to call num_minutes() or else comparison doesn't work as expected
        self.elapsed_minutes().num_minutes() > self.time_box.num_minutes()
    }

    pub fn exercise_display_str(
        &self,
        i: usize,
        exercise_cnt: usize,
        exercise: &Exercise,
    ) -> String {
        // right align timebox
        let exercise_str = format!(
            "Exercise {}/{} - ID {}",
            i + 1,
            exercise_cnt,
            &exercise.id.unwrap_or(-1)
        );
        let review_session_timebox_str = format!("{}", self);
        let pad_width = std::cmp::max(
            1,
            STANDARD_REVIEW_OUTPUT_WIDTH - exercise_str.len() - review_session_timebox_str.len(),
        );

        let pad_str = " ".repeat(pad_width);

        format!(
            "{}{}{}\n",
            exercise_str, pad_str, review_session_timebox_str,
        )
    }
}

impl fmt::Display for ReviewSession {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let elapsed_minutes = self.elapsed_minutes();

        if self.has_exceeded_timebox() {
            let excess_minutes = (elapsed_minutes - self.time_box).num_minutes();
            write!(f, "<Overtime: {}m>", excess_minutes)
        } else {
            write!(
                f,
                "{}m/{}m",
                elapsed_minutes.num_minutes(),
                self.time_box.num_minutes()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_session_elapsed_minutes() {
        let mut session = ReviewSession::new(None);
        assert_eq!(
            session.time_box_minutes(),
            REVIEW_SESSION_TIME_BOX_DEFAULT_MINUTES
        );
        assert_eq!(session.elapsed_minutes().num_minutes(), 0);

        session.start = session.start - Duration::minutes(30);

        // this could be flaky if we start the test right near the end of a
        // minute but it's unlikely
        assert_eq!(session.elapsed_minutes().num_minutes(), 30);
    }

    #[test]
    fn test_review_session_has_exceeded_timebox() {
        let mut session = ReviewSession::new(None);
        assert!(!session.has_exceeded_timebox());

        session.start = session.start - Duration::minutes(30);

        // this could be flaky if we start the test right near the end of a
        // minute but it's unlikely

        assert!(session.has_exceeded_timebox());

        session.time_box = Duration::minutes(31);

        assert!(!session.has_exceeded_timebox());

        session.start = session.start - Duration::minutes(32);

        assert!(session.has_exceeded_timebox());
    }

    #[test]
    fn test_review_session_review_session_formatting() {
        let mut session = ReviewSession::new(None);

        let session_str = format!("{}", session);
        assert_eq!("0m/20m", session_str);

        let mut exercise = Exercise::new("foo", "bar", "baz");
        exercise.id = Some(1234);

        let exercise_display_str = session.exercise_display_str(0, 10, &exercise);
        assert_eq!(
            exercise_display_str,
            "Exercise 1/10 - ID 1234                                                   0m/20m\n"
        );

        session.start = session.start - Duration::minutes(5);

        let session_str = format!("{}", session);
        assert_eq!("5m/20m", session_str);

        session.start = session.start - Duration::minutes(15);

        let session_str = format!("{}", session);
        assert_eq!("20m/20m", session_str);

        session.start = session.start - Duration::minutes(1);

        let session_str = format!("{}", session);
        assert_eq!("<Overtime: 1m>", session_str);

        session.start = session.start - Duration::minutes(10);

        let session_str = format!("{}", session);
        assert_eq!("<Overtime: 11m>", session_str);
    }
}
