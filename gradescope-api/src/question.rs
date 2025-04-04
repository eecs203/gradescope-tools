use std::num::NonZeroU8;
use std::str::FromStr;
use std::{fmt, iter};

use itertools::{Either, Itertools};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AssignmentOutline {
    pub outline: Outline,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct Outline {
    questions: Vec<OutlineQuestionTree>,
}

impl Outline {
    pub fn into_questions(self) -> impl Iterator<Item = Question> {
        self.questions
            .into_iter()
            .flat_map(|question| question.into_questions(Vec::new()))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum OutlineQuestionTree {
    #[serde(rename = "QuestionGroup")]
    Inner {
        index: NonZeroU8,
        children: Vec<OutlineQuestionTree>,
    },
    #[serde(rename = "FreeResponseQuestion")]
    Leaf {
        title: QuestionTitle,
        index: NonZeroU8,
    },
}

impl OutlineQuestionTree {
    fn into_questions(self, mut num_prefix: Vec<NonZeroU8>) -> impl Iterator<Item = Question> {
        match self {
            Self::Inner {
                index, children, ..
            } => {
                num_prefix.push(index);
                let iter = children
                    .into_iter()
                    .flat_map(move |child| child.into_questions(num_prefix.clone()));
                Either::Left(Box::new(iter) as Box<dyn Iterator<Item = Question>>)
            }
            Self::Leaf { title, index } => {
                num_prefix.push(index);
                let number = QuestionNumber::new(num_prefix);
                let question = Question { title, number };
                Either::Right(iter::once(question))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Question {
    title: QuestionTitle,
    number: QuestionNumber,
}

impl Question {
    pub fn title(&self) -> &QuestionTitle {
        &self.title
    }

    pub fn number(&self) -> &QuestionNumber {
        &self.number
    }
}

impl fmt::Display for Question {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.number, self.title)
    }
}

// Not just an integer because of question parts. For example, part 2 of question 3 is "3.2".
// TODO: parse as a sequence of integers
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct QuestionNumber {
    numbers: Vec<NonZeroU8>,
}

impl QuestionNumber {
    pub fn new(numbers: Vec<NonZeroU8>) -> Self {
        Self { numbers }
    }

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

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(transparent)]
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
