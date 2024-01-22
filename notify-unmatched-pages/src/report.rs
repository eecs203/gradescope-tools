use core::fmt;

use anyhow::{Context, Result};
use gradescope_api::assignment::{Assignment, AssignmentClient, AssignmentId, AssignmentName};
use gradescope_api::course::{Course, CourseId};
use gradescope_api::question::QuestionNumber;
use gradescope_api::submission::{StudentSubmitter, SubmissionId};
use gradescope_api::types::{Email, StudentName};
use gradescope_api::unmatched::{NonmatchingSubmitter, UnmatchedQuestion};
use itertools::Itertools;
use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::{Address, AsyncSendmailTransport, Message};

use crate::sender::Sender;

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

    pub fn email(&self) -> &Email {
        &self.email
    }

    pub fn mailbox(&self) -> Result<Mailbox> {
        let address = self
            .email
            .to_string()
            .parse()
            .context("could not parse student email")?;
        let name = self.name().to_string();
        Ok(Mailbox::new(Some(name), address))
    }
}

impl fmt::Display for UnmatchedStudent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.email)
    }
}

#[derive(Debug, Clone)]
pub struct UnmatchedQuestions {
    questions: Vec<UnmatchedQuestion>,
}

impl UnmatchedQuestions {
    pub fn new(questions: impl Iterator<Item = UnmatchedQuestion>) -> Self {
        Self {
            questions: questions.collect(),
        }
    }

    pub fn questions(&self) -> &[UnmatchedQuestion] {
        &self.questions
    }
}

impl fmt::Display for UnmatchedQuestions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.questions.len() {
            0 => write!(f, "no questions"),
            1 => self.questions[0].fmt(f),
            _ => {
                write!(f, "\n  - {}", self.questions.iter().format("\n  - "))
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
    pub fn new(client: &'a AssignmentClient, nonmatching_submitter: NonmatchingSubmitter) -> Self {
        let student = UnmatchedStudent::new(nonmatching_submitter.student());
        let submission = nonmatching_submitter.submission();
        let submission_id = submission.id().clone();
        let unmatched = UnmatchedQuestions::new(submission.questions().iter().cloned());

        Self {
            course_id: client.course().id(),
            assignment_id: client.assignment().id(),
            submission_id,
            assignment_name: client.assignment().name(),
            student,
            unmatched,
        }
    }

    pub fn send_as_email(&self, sender: &Sender) -> Result<()> {
        // let message = Message::builder()
        //     .from(sender.from().clone())
        //     .to(self.student.mailbox()?)
        //     .subject("Page Matching Notification")
        //     .header(ContentType::TEXT_PLAIN)
        //     .body(body);
        // let mailer = AsyncSendmailTransport::new();
        todo!()
    }

    pub fn page_matching_link(&self) -> String {
        format!(
            "https://www.gradescope.com/courses/{}/assignments/{}/submissions/{}/select_pages",
            self.course_id, self.assignment_id, self.submission_id
        )
    }

    pub fn csv_string(&self) -> String {
        let (questions, these, them) = if self.unmatched.questions().len() == 1 {
            // Singular
            ("question", "this", "it")
        } else {
            // Plural
            ("questions", "these", "them")
        };

        format!(
            "{};{};We found {} unmatched {questions} in your submission for {}: {}",
            self.student.name(),
            self.student.email(),
            self.unmatched.questions().len(),
            self.assignment_name,
            self.unmatched,
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
