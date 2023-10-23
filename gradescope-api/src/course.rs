use std::fmt;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_with::serde_conv;

use crate::assignment::{Assignment, AssignmentClient};
use crate::client::{Auth, Client};

#[derive(Debug, Clone)]
pub struct Course {
    id: CourseId,
    short_name: String,
    name: String,
    user_role: Role,
}

impl Course {
    pub fn new(id: CourseId, short_name: String, name: String, user_role: Role) -> Self {
        Self {
            id,
            short_name,
            name,
            user_role,
        }
    }

    pub fn id(&self) -> &CourseId {
        &self.id
    }

    pub fn short_name(&self) -> &str {
        &self.short_name
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn user_role(&self) -> Role {
        self.user_role
    }

    pub fn find_by_short_name(
        name: &str,
        courses: impl IntoIterator<Item = Self>,
    ) -> Result<Course> {
        let pred = |course: &Course| course.short_name() == name;
        courses
            .into_iter()
            .find(pred)
            .with_context(|| format!("could not find course with short name \"{name}\""))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CourseClient<'a> {
    gradescope: &'a Client<Auth>,
    course: &'a Course,
}

impl<'a> CourseClient<'a> {
    pub fn new(gradescope: &'a Client<Auth>, course: &'a Course) -> Self {
        Self { gradescope, course }
    }

    pub fn with_assignment(&self, assignment: &'a Assignment) -> AssignmentClient<'a> {
        AssignmentClient::new(*self, assignment)
    }

    pub fn gradescope(&self) -> &'a Client<Auth> {
        self.gradescope
    }

    pub fn course(&self) -> &'a Course {
        self.course
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(transparent)]
pub struct CourseId {
    id: String,
}

impl CourseId {
    pub fn new(id: String) -> Self {
        Self { id }
    }

    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for CourseId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

serde_conv! {
    pub(crate) CourseIdAsInt,
    CourseId,
    |course_id: &CourseId| course_id.id.parse::<u64>().unwrap(),
    |value: u64| -> Result<_, std::convert::Infallible> {
        Ok(CourseId {
            id: value.to_string(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Student,
    Instructor,
}
