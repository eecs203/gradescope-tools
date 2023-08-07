//! Holds types that don't "do" much (at least at present), especially when it would be difficult to
//! place them before further building out the Gradescope data model.

use std::fmt;
use std::num::FpCategory;

use anyhow::{bail, Result};
use serde::Serialize;

// Not just an integer because of question parts. For example, part 2 of question 3 is "3.2".
// TODO: parse as a sequence of integers
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct QuestionNumber {
    number: String,
}

impl QuestionNumber {
    pub fn new(number: String) -> Self {
        Self { number }
    }

    pub fn as_str(&self) -> &str {
        &self.number
    }
}

impl fmt::Display for QuestionNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.number.fmt(f)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct QuestionTitle {
    title: String,
}

impl QuestionTitle {
    pub fn new(title: String) -> Self {
        Self { title }
    }

    pub fn as_str(&self) -> &str {
        &self.title
    }
}

impl fmt::Display for QuestionTitle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.title.fmt(f)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct GraderName {
    name: String,
}

impl GraderName {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for GraderName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.name.fmt(f)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct StudentName {
    name: String,
}

impl StudentName {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for StudentName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.name.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Points {
    points: f32,
}

impl Points {
    pub fn new(points: f32) -> Result<Self> {
        match points.classify() {
            FpCategory::Normal => Ok(Self { points }),
            category => bail!("attempted to construct points with value `{points}`, which has non-normal category `{category:?}`"),
        }
    }

    pub fn as_f32(self) -> f32 {
        self.points
    }
}
