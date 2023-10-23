use core::fmt;

use gradescope_api::assignment::{Assignment, AssignmentId, AssignmentName};
use gradescope_api::course::{Course, CourseId};
use gradescope_api::submission::{StudentSubmitter, SubmissionId};
use gradescope_api::types::{Email, QuestionNumber, StudentName};
use gradescope_api::unmatched::{NonmatchingSubmitter, UnmatchedQuestion};
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
pub struct UnmatchedQuestions {
    questions: Vec<QuestionNumber>,
}

impl UnmatchedQuestions {
    pub fn new(questions: impl Iterator<Item = QuestionNumber>) -> Self {
        Self {
            questions: questions.collect(),
        }
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
pub struct UnmatchedReport<'a> {
    course_id: &'a CourseId,
    assignment_id: &'a AssignmentId,
    assignment_name: &'a AssignmentName,
    submission_id: SubmissionId,
    student: UnmatchedStudent,
    unmatched: UnmatchedQuestions,
}

impl<'a> UnmatchedReport<'a> {
    pub fn new(
        course: &'a Course,
        assignment: &'a Assignment,
        nonmatching_submitter: NonmatchingSubmitter,
    ) -> Self {
        let student = UnmatchedStudent::new(nonmatching_submitter.student());
        let submission = nonmatching_submitter.submission();
        let submission_id = submission.id().clone();
        let unmatched = UnmatchedQuestions::new(
            submission
                .questions()
                .iter()
                .map(UnmatchedQuestion::question)
                .cloned(),
        );

        Self {
            course_id: course.id(),
            assignment_id: assignment.id(),
            submission_id,
            assignment_name: assignment.name(),
            student,
            unmatched,
        }
    }

    pub fn page_matching_link(&self) -> String {
        format!(
            "https://www.gradescope.com/courses/{}/assignments/{}/submissions/{}/select_pages",
            self.course_id, self.assignment_id, self.submission_id
        )
    }
}

impl<'a> fmt::Display for UnmatchedReport<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (questions, these, them) = if self.unmatched.questions().len() == 1 {
            // Singular
            ("question", "this", "it")
        } else {
            // Plural
            ("questions", "these", "them")
        };

        write!(
            f,
            "{}:\n\nWe found {} unmatched {questions} in your submission for {}: {}\n\nIf you would like {these} {questions} to be graded, please match pages for {them} as soon as possible.\n\n- EECS 203",
            self.student, self.unmatched.questions().len(), self.assignment_name, self.unmatched,
        )
    }
}
