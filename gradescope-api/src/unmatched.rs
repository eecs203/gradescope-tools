use std::iter;

use anyhow::{Context as AnyhowContext, Result};
use futures::{stream, Stream, TryStreamExt};
use itertools::Either;

use crate::submission::{StudentSubmitter, SubmissionId, SubmissionToStudentMap};
use crate::types::QuestionNumber;

/// An `UnmatchedQuestion` is a question in a submission without any pages matched to it.
#[derive(Debug, Clone)]
pub struct UnmatchedQuestion {
    question: QuestionNumber,
}

impl UnmatchedQuestion {
    pub fn new(question: QuestionNumber) -> Self {
        Self { question }
    }

    pub fn question(&self) -> &QuestionNumber {
        &self.question
    }
}

/// An `UnmatchedSubmission` is a submission that has at least one `UnmatchedQuestion`.
#[derive(Debug, Clone)]
pub struct UnmatchedSubmission {
    id: SubmissionId,
    unmatched_questions: Vec<UnmatchedQuestion>,
}

impl UnmatchedSubmission {
    pub fn new(id: SubmissionId, unmatched_questions: Vec<UnmatchedQuestion>) -> Self {
        Self {
            id,
            unmatched_questions,
        }
    }

    pub fn id(&self) -> &SubmissionId {
        &self.id
    }

    pub fn questions(&self) -> &[UnmatchedQuestion] {
        &self.unmatched_questions
    }

    pub fn submitters(
        self,
        submission_to_student_map: SubmissionToStudentMap,
    ) -> impl Iterator<Item = Result<NonmatchingSubmitter>> {
        let students = submission_to_student_map
            .students(&self.id)
            .context("could not find students for submission");
        match students {
            Ok(students) => Either::Left(
                students
                    .to_vec()
                    .into_iter()
                    .map(move |student| NonmatchingSubmitter::new(student, self.clone()))
                    .map(Ok),
            ),
            Err(err) => Either::Right(iter::once(Err(err))),
        }
    }
}

fn id<A, B>(f: impl Fn(A) -> B) -> impl Fn(A) -> B {
    f
}

pub trait UnmatchedSubmissionStream: Stream<Item = Result<UnmatchedSubmission>> + Sized {
    fn submitters(
        self,
        submission_to_student_map: SubmissionToStudentMap,
    ) -> impl Stream<Item = Result<NonmatchingSubmitter>> {
        self.map_ok(id(move |unmatched_submission: UnmatchedSubmission| {
            unmatched_submission.submitters(submission_to_student_map.clone())
        }))
        .map_ok(stream::iter)
        .try_flatten()
    }
}

impl<S: Stream<Item = Result<UnmatchedSubmission>>> UnmatchedSubmissionStream for S {}

#[derive(Debug, Clone)]
pub struct NonmatchingSubmitter {
    student: StudentSubmitter,
    submission: UnmatchedSubmission,
}

impl NonmatchingSubmitter {
    pub fn new(student: StudentSubmitter, submission: UnmatchedSubmission) -> Self {
        Self {
            student,
            submission,
        }
    }

    pub fn student(&self) -> &StudentSubmitter {
        &self.student
    }

    pub fn submission(&self) -> &UnmatchedSubmission {
        &self.submission
    }
}
