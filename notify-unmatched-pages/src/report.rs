use core::fmt;
use std::ops::Deref;

use gradescope_api::submission::{StudentSubmitter, SubmissionId};
use gradescope_api::types::{Email, StudentName};
use itertools::Itertools;

#[derive(Debug, Clone)]
pub struct UnmatchedStudent {
    name: StudentName,
    email: Email,
}

impl UnmatchedStudent {
    pub fn new(student_submitter: &StudentSubmitter) -> Self {
        let name = student_submitter.name().clone();
        let email = student_submitter.email().clone();
        Self { name, email }
    }
}

impl fmt::Display for UnmatchedStudent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.email)
    }
}

#[derive(Debug, Clone)]
pub struct UnmatchedStudents {
    students: Vec<UnmatchedStudent>,
}

impl UnmatchedStudents {
    pub fn new(students: &[&StudentSubmitter]) -> Self {
        let students = students
            .iter()
            .map(Deref::deref)
            .map(UnmatchedStudent::new)
            .collect();

        Self { students }
    }
}

impl fmt::Display for UnmatchedStudents {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.students.len() {
            0 => write!(f, "no students"),
            1 => self.students[0].fmt(f),
            2 => write!(f, "{} and {}", &self.students[0], &self.students[1]),
            n => {
                let first_students = self.students.iter().take(n - 1);
                write!(
                    f,
                    "{}, and {}",
                    first_students.format(", "),
                    &self.students[n - 1],
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnmatchedReport {
    submission_id: SubmissionId,
    students: UnmatchedStudents,
    num_unmatched: usize,
}

impl UnmatchedReport {
    pub fn new(
        (submission_id, students, num_unmatched): (SubmissionId, &Vec<&StudentSubmitter>, usize),
    ) -> Self {
        let students = UnmatchedStudents::new(students);

        Self {
            submission_id,
            students,
            num_unmatched,
        }
    }
}

impl fmt::Display for UnmatchedReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}:\n\nWe found {} unmatched questions in your submission {}. If you would like these questions to be graded, please match pages for them as soon as possible.\n\n- EECS 203",
            self.students, self.num_unmatched, self.submission_id
        )
    }
}
