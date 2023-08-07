use std::fmt;

use crate::types::Points;

#[derive(Debug, Clone)]
pub struct Assignment {
    id: String,
    name: AssignmentName,
    points: Points,
}

impl Assignment {
    pub fn new(id: String, name: AssignmentName, points: Points) -> Self {
        Self { id, name, points }
    }

    pub fn id(&self) -> &str {
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
