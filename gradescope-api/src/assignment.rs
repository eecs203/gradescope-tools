use std::fmt;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, serde_conv};

use crate::course::{Course, CourseClient};
use crate::question::Outline;
use crate::submission::SubmissionToStudentMap;
use crate::submission_export::SubmissionExport;
use crate::types::Points;

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    #[serde_as(as = "AssignmentIdWithUnderscore")]
    id: AssignmentId,
    #[serde(rename = "title")]
    name: AssignmentName,
    #[serde(rename = "total_points")]
    points: Points,
}

impl Assignment {
    pub fn new(id: AssignmentId, name: AssignmentName, points: Points) -> Self {
        Self { id, name, points }
    }

    pub fn id(&self) -> &AssignmentId {
        &self.id
    }

    pub fn name(&self) -> &AssignmentName {
        &self.name
    }

    pub fn points(&self) -> Points {
        self.points
    }
}

pub struct AssignmentClient<'a> {
    course_client: CourseClient<'a>,
    assignment: &'a Assignment,
}

impl<'a> AssignmentClient<'a> {
    pub fn new(course_client: CourseClient<'a>, assignment: &'a Assignment) -> Self {
        Self {
            course_client,
            assignment,
        }
    }

    pub fn course(&self) -> &'a Course {
        self.course_client.course()
    }

    pub fn assignment(&self) -> &'a Assignment {
        self.assignment
    }

    pub async fn download_submission_export(&self) -> Result<impl SubmissionExport> {
        let gradescope = self.course_client.gradescope();
        let course = self.course_client.course();

        let export = gradescope
            .export_submissions(course, self.assignment)
            .await?;

        Ok(export)
    }

    pub async fn outline(&self) -> Result<Outline> {
        let gradescope = self.course_client.gradescope();
        let course = self.course_client.course();

        let outline = gradescope.get_outline(course, self.assignment).await?;

        Ok(outline)
    }

    pub async fn submission_to_student_map(&self) -> Result<SubmissionToStudentMap> {
        let gradescope = self.course_client.gradescope();
        let course = self.course_client.course();

        let submission_to_student_map = gradescope
            .get_submissions_metadata(course, self.assignment)
            .await?
            .submission_to_student_map()?;

        Ok(submission_to_student_map)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssignmentId {
    id: String,
}

impl AssignmentId {
    pub fn new(id: String) -> Self {
        Self { id }
    }

    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for AssignmentId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

serde_conv! {
    pub(crate) AssignmentIdAsInt,
    AssignmentId,
    |assignment_id: &AssignmentId| assignment_id.id.parse::<u64>().unwrap(),
    |value: u64| -> Result<_, std::convert::Infallible> {
        Ok(AssignmentId {
            id: value.to_string(),
        })
    }
}

serde_conv! {
    pub(crate) AssignmentIdWithUnderscore,
    AssignmentId,
    |assignment_id: &AssignmentId| format!("assignment_{}", assignment_id.id),
    |value: &str| -> Result<_, std::convert::Infallible> {
        Ok(AssignmentId {
            id: value.trim_start_matches("assignment_").to_string(),
        })
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssignmentName {
    name: String,
}

impl AssignmentName {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for AssignmentName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.name.fmt(f)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AssignmentsTableProps {
    #[serde(rename = "table_data")]
    pub assignments: Vec<Assignment>,
}
