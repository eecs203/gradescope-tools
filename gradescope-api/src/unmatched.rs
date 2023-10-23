use std::iter;
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::{Context as AnyhowContext, Result};
use futures::{stream, Stream, TryStreamExt};
use itertools::Either;
use pin_project::pin_project;

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

#[pin_project]
pub struct UnmatchedSubmissionStream<S: Stream<Item = Result<UnmatchedSubmission>>> {
    #[pin]
    stream: S,
}

fn id<A, B>(f: impl Fn(A) -> B) -> impl Fn(A) -> B {
    f
}

impl<'a, S: Stream<Item = Result<UnmatchedSubmission>> + 'a> UnmatchedSubmissionStream<S> {
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    pub fn submitters(
        self,
        submission_to_student_map: SubmissionToStudentMap,
    ) -> impl Stream<Item = Result<NonmatchingSubmitter>> {
        self.stream
            .map_ok(id(move |unmatched_submission: UnmatchedSubmission| {
                unmatched_submission.submitters(submission_to_student_map.clone())
            }))
            .map_ok(stream::iter)
            .try_flatten()
    }
}

impl<S: Stream<Item = Result<UnmatchedSubmission>>> Stream for UnmatchedSubmissionStream<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

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
