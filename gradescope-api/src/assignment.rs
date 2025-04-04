use std::io::Write;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, serde_conv};

use crate::client::Client;
use crate::course::{Course, CourseClient};
use crate::question::Outline;
use crate::services::gs_service::GsService;
use crate::submission::SubmissionToStudentMap;
use crate::submission_export::{SubmissionExport, load_submissions_export_from_fs};
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

pub struct AssignmentClient<'a, Service> {
    course_client: CourseClient<'a, Service>,
    assignment: &'a Assignment,
}

impl<'a, Service: GsService> AssignmentClient<'a, Service> {
    pub fn new(course_client: CourseClient<'a, Service>, assignment: &'a Assignment) -> Self {
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

    fn gradescope(&self) -> &'a Client<Service> {
        self.course_client.gradescope()
    }

    pub fn get_cache_path(&self) -> &Path {
        self.course_client.get_cache_path()
    }

    /// The path on the filesystem where the submission export for this assignment is/will be cached
    pub fn get_submission_export_path(&self) -> PathBuf {
        let course = self.course().name();
        let name = self.assignment().name().as_str();
        self.get_cache_path()
            .join(format!("{course}-{name}-export.zip"))
    }

    /// Once the submission export has been cached to the filesystem, load it into a usable object
    pub async fn load_submission_export_from_fs(
        &self,
    ) -> Result<impl SubmissionExport + use<Service>> {
        load_submissions_export_from_fs(self.get_submission_export_path()).await
    }

    /// Get the path on the filesystem to the submissions export, possibly exporting the submissions
    /// if not already present.
    pub async fn ensure_submissions_export_on_fs(&self) -> Result<PathBuf> {
        let path = self.get_submission_export_path();
        if path.exists() {
            // The export is already in cache
            return Ok(path);
        }

        self.export_submissions_to_fs().await
    }

    /// Export the submissions and save them to the filesystem
    async fn export_submissions_to_fs(&self) -> Result<PathBuf> {
        let mut submissions_response = self
            .gradescope()
            .submission_export_response(self.course(), self.assignment())
            .await?;

        let path = self.get_submission_export_path();
        let tmp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&tmp_path)?;

        while let Some(data) = submissions_response.chunk().await? {
            file.write_all(&data)?;
        }

        std::fs::rename(tmp_path, &path)?;

        Ok(path)
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
