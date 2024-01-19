//! Holds types that don't "do" much (at least at present), especially when it would be difficult to
//! place them before further building out the Gradescope data model.

use std::fmt;
use std::num::{FpCategory, NonZeroU8};
use std::str::FromStr;

use anyhow::{bail, Result};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, serde_conv, DisplayFromStr};

// Not just an integer because of question parts. For example, part 2 of question 3 is "3.2".
// TODO: parse as a sequence of integers
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct QuestionNumber {
    numbers: Vec<NonZeroU8>,
}

impl QuestionNumber {
    /// Assuming this question number is for a leaf (i.e. it has no parts, subparts, ...),
    /// determines if this is the first question. If it is not a leaf, determines if this is the
    /// first question at its level.
    pub fn is_first(&self) -> bool {
        self.numbers.iter().all(|number| *number == NonZeroU8::MIN)
    }
}

impl FromStr for QuestionNumber {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Question numbers are of the form `n_0.n_1.n_2.…`, where `n_0` is the top-level question
        // number, `n_1` is the question part, `n_2` is the subpart...
        s.split('.')
            .map(NonZeroU8::from_str)
            .try_collect()
            .map(|numbers| Self { numbers })
    }
}

impl fmt::Debug for QuestionNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("QuestionNumber")
            .field(&format_args!("{}", self))
            .finish()
    }
}

impl fmt::Display for QuestionNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.numbers.iter().format("."))
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

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(transparent)]
pub struct StudentId {
    id: String,
}

impl fmt::Display for StudentId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

serde_conv! {
    pub(crate) StudentIdAsInt,
    StudentId,
    |student_id: &StudentId| student_id.id.parse::<u64>().unwrap(),
    |value: u64| -> Result<_, std::convert::Infallible> {
        Ok(StudentId {
            id: value.to_string(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct Email {
    email: String,
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.email.fmt(f)
    }
}

#[serde_as]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Points {
    #[serde_as(as = "DisplayFromStr")]
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
