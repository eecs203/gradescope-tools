use core::fmt;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use itertools::Itertools;
use serde::Deserialize;
use serde_with::{serde_as, serde_conv};
use tracing::warn;

use crate::assignment::{AssignmentId, AssignmentIdAsInt};
use crate::types::{Email, StudentId, StudentIdAsInt, StudentName};

#[derive(Debug, Clone, Hash, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct SubmissionId {
    id: String,
}

impl SubmissionId {
    pub fn new(id: String) -> Self {
        Self { id }
    }
}

impl fmt::Display for SubmissionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

serde_conv! {
    pub(crate) SubmissionIdAsInt,
    SubmissionId,
    |submission_id: &SubmissionId| submission_id.id.parse::<u64>().unwrap(),
    |value: u64| -> Result<_, std::convert::Infallible> {
        Ok(SubmissionId {
            id: value.to_string(),
        })
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionsManagerProps {
    #[serde_as(as = "AssignmentIdAsInt")]
    assignment_id: AssignmentId,
    students: Vec<StudentSubmitter>,
    submissions: HashMap<SubmissionId, Submission>,
}

impl SubmissionsManagerProps {
    pub fn assignment_id(&self) -> &AssignmentId {
        &self.assignment_id
    }

    fn id_to_student_map(&self) -> HashMap<StudentId, &StudentSubmitter> {
        self.students
            .iter()
            .map(|student| (student.id.clone(), student))
            .collect()
    }

    pub fn submission_to_student_map(&self) -> Result<SubmissionToStudentMap> {
        let id_to_student = self.id_to_student_map();

        let map: HashMap<_, _> = self.submissions
            .iter()
            .map(|(id, submission)| {
                if id == &submission.id {
                    Ok((id, submission))
                } else {
                    Err(anyhow!(
                        "submission with key `{id:?}` has mismatching id `{:?}`",
                        submission.id
                    ))
                }
            })
            .map(|result| match result {
                Ok((id, submission)) => {
                    let students = submission
                        .active_user_ids
                        .iter()
                        .filter_map(|id| {
                            let student = id_to_student.get(id).copied().cloned();
                            if student.is_none() {
                                warn!("could not find student with id {id} for submission {submission:?}; they were likely removed from the roster");
                            }
                            student
                        })
                        .collect_vec();
                    Ok((id.clone(), students))
                }
                Err(err) => Err(err),
            })
            .try_collect()?;

        Ok(SubmissionToStudentMap::new(map))
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct StudentSubmitter {
    #[serde_as(as = "StudentIdAsInt")]
    id: StudentId,
    name: StudentName,
    email: Email,
}

impl StudentSubmitter {
    pub fn name(&self) -> &StudentName {
        &self.name
    }

    pub fn email(&self) -> &Email {
        &self.email
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
struct Submission {
    #[serde_as(as = "SubmissionIdAsInt")]
    id: SubmissionId,
    #[serde_as(as = "Vec<StudentIdAsInt>")]
    active_user_ids: Vec<StudentId>,
}

#[derive(Debug, Clone)]
pub struct SubmissionToStudentMap(Arc<HashMap<SubmissionId, Vec<StudentSubmitter>>>);

impl SubmissionToStudentMap {
    pub fn new(map: HashMap<SubmissionId, Vec<StudentSubmitter>>) -> Self {
        Self(Arc::new(map))
    }

    pub fn students<'a>(&'a self, submission_id: &SubmissionId) -> Option<&'a [StudentSubmitter]> {
        self.0.get(submission_id).map(Vec::as_slice)
    }
}
