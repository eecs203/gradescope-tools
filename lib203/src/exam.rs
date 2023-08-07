use std::collections::HashMap;
use std::ops::Deref;

use anyhow::Result;
use futures::{stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::Assignment;
use gradescope_api::client::{Auth, Client};
use gradescope_api::course::Course;
use gradescope_api::regrade::Regrade;
use itertools::Itertools;

pub fn find_exams(assignments: &[Assignment]) -> HashMap<&str, Vec<Exam>> {
    let exams = Exam::get_from(assignments);
    exams.into_group_map_by(|exam| exam.number())
}

pub async fn get_exam_regrades<'a>(
    exams: &HashMap<&'a str, Vec<Exam<'a>>>,
    gradescope: &Client<Auth>,
    course: &Course,
) -> Result<HashMap<&'a str, Vec<(Exam<'a>, Vec<Regrade>)>>> {
    let get_regrades = |assignment| gradescope.get_regrades(course, assignment);

    stream::iter(exams)
        .then(|(num, exams)| async move {
            let regrades = stream::iter(exams).map(Deref::deref).then(get_regrades);
            stream::iter(exams)
                .map(Clone::clone)
                .zip(regrades)
                .map(|(x, y)| y.map(|y| (x, y)))
                .try_collect::<Vec<_>>()
                .await
                .map(|x| (*num, x))
        })
        .try_collect()
        .await
}

#[derive(Debug, Clone, Copy)]
pub struct Exam<'a> {
    assignment: &'a Assignment,
    number: &'a str,
    kind: Option<ExamKind>,
}

#[derive(Debug, Clone, Copy)]
pub enum ExamKind {
    Regular,
    Alternate,
}

impl<'a> Exam<'a> {
    pub fn get_from(
        assignments: impl IntoIterator<Item = &'a Assignment>,
    ) -> impl Iterator<Item = Exam<'a>> {
        assignments
            .into_iter()
            .map(TryFrom::try_from)
            .filter_map(Result::ok)
    }

    pub fn number(&self) -> &'a str {
        self.number
    }

    pub fn kind(&self) -> Option<ExamKind> {
        self.kind
    }
}

impl<'a> TryFrom<&'a Assignment> for Exam<'a> {
    type Error = ();

    fn try_from(assignment: &'a Assignment) -> Result<Self, Self::Error> {
        let name = assignment.name().as_str().strip_prefix("Exam ").ok_or(())?;

        if name.contains("Ungraded") {
            return Err(());
        }

        let (number, kind) = if let Some(number) = name.strip_suffix(" Regular") {
            (number, Some(ExamKind::Regular))
        } else if let Some(number) = name.strip_suffix(" Alternate") {
            (number, Some(ExamKind::Alternate))
        } else {
            (name, None)
        };

        Ok(Self {
            assignment,
            number,
            kind,
        })
    }
}

impl<'a> Deref for Exam<'a> {
    type Target = Assignment;

    fn deref(&self) -> &Self::Target {
        self.assignment
    }
}
