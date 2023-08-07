use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Course {
    id: String,
    short_name: String,
    name: String,
    user_role: Role,
}

impl Course {
    pub fn new(id: String, short_name: String, name: String, user_role: Role) -> Self {
        Self {
            id,
            short_name,
            name,
            user_role,
        }
    }

    pub fn id(&self) -> &str {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Student,
    Instructor,
}
