use std::fmt;

use serde::Deserialize;
use serde_with::serde_conv;

use crate::types::Points;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
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

#[derive(Debug, Clone)]
pub struct Assignment {
    id: AssignmentId,
    name: AssignmentName,
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

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
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
