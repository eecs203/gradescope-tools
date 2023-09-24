use core::fmt;

use gradescope_api::assignment::AssignmentName;
use gradescope_api::submission::{StudentSubmitter, SubmissionId};
use gradescope_api::types::{Email, QuestionNumber, StudentName};
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

    pub fn name(&self) -> &StudentName {
        &self.name
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
    pub fn new(students: &[StudentSubmitter]) -> Self {
        let students = students.iter().map(UnmatchedStudent::new).collect();

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
pub struct UnmatchedQuestions {
    questions: Vec<QuestionNumber>,
}

impl UnmatchedQuestions {
    pub fn new(questions: Vec<QuestionNumber>) -> Self {
        Self { questions }
    }

    pub fn questions(&self) -> &[QuestionNumber] {
        &self.questions
    }
}

impl fmt::Display for UnmatchedQuestions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.questions.len() {
            0 => write!(f, "no questions"),
            1 => self.questions[0].fmt(f),
            2 => write!(f, "{} and {}", &self.questions[0], &self.questions[1]),
            n => {
                let first_students = self.questions.iter().take(n - 1);
                write!(
                    f,
                    "{}, and {}",
                    first_students.format(", "),
                    &self.questions[n - 1],
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnmatchedReport {
    submission_id: SubmissionId,
    assignment_name: AssignmentName,
    students: UnmatchedStudents,
    unmatched: UnmatchedQuestions,
}

impl UnmatchedReport {
    pub fn new(
        submission_id: SubmissionId,
        assignment_name: AssignmentName,
        students: &[StudentSubmitter],
        unmatched: Vec<QuestionNumber>,
    ) -> Self {
        let students = UnmatchedStudents::new(students);
        let unmatched = UnmatchedQuestions::new(unmatched);

        Self {
            submission_id,
            assignment_name,
            students,
            unmatched,
        }
    }

    pub fn students(&self) -> &[UnmatchedStudent] {
        &self.students.students
    }
}

impl fmt::Display for UnmatchedReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}:\n\nWe found {} unmatched question(s) in your submission for {}: {}\n\nIf you would like these questions to be graded, please match pages for them as soon as possible.\n\n- EECS 203",
            self.students, self.unmatched.questions().len(), self.assignment_name, self.unmatched,
        )
    }
}
