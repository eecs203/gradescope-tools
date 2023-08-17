use std::collections::HashMap;

use anyhow::{anyhow, Result};
use itertools::Itertools;
use serde::Deserialize;
use serde_with::{serde_as, serde_conv};
use tracing::warn;

use crate::assignment::{AssignmentId, AssignmentIdAsInt};
use crate::types::{StudentId, StudentIdAsInt, StudentName};

#[derive(Debug, Clone, Hash, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct SubmissionId {
    id: String,
}

serde_conv!(
    pub(crate) SubmissionIdAsInt,
    SubmissionId,
    |submission_id: &SubmissionId| submission_id.id.parse::<u64>().unwrap(),
    |value: u64| -> Result<_, std::convert::Infallible> {
        Ok(SubmissionId {
            id: value.to_string(),
        })
    }
);

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

    pub fn submission_to_student_map(
        &self,
    ) -> Result<HashMap<SubmissionId, Vec<&StudentSubmitter>>> {
        let id_to_student = self.id_to_student_map();

        self.submissions
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
                            let student = id_to_student
                                .get(id)
                                .copied();
                            if student.is_none() {
                                warn!("could not find student with id {id} for submission {submission:?}; they were likely removed from the roster");
                            }
                            student
                        }).collect();
                    Ok((id.clone(), students))
                }
                Err(err) => Err(err),
            })
            .try_collect()
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct StudentSubmitter {
    #[serde_as(as = "StudentIdAsInt")]
    id: StudentId,
    name: StudentName,
    email: String,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
struct Submission {
    #[serde_as(as = "SubmissionIdAsInt")]
    id: SubmissionId,
    #[serde_as(as = "Vec<StudentIdAsInt>")]
    active_user_ids: Vec<StudentId>,
}
