//! Processing general assignments as EECS 203 homeworks.
//!
//! Each assignment that is a homework is either Individual or Groupwork and has a number. For each
//! homework number, there is usually an individual and group assignment, but not necessarily. In
//! practice, there have been individual homework 0s with no associated groupwork, and the last
//! homeworks may not have graded groupworks.
//!
//! The homework number is expected to be a nonnegative integer, but is represented as a string in
//! case this changes.
//!
//! # Terminology:
//! - Homework/HW: an assignment that is a homework, including both individual and groupwork
//! - Individual/ID, Groupwork/GW: an assignment that is an individual/groupwork homework
//! - Homework Pair/HW Pair: all homeworks with the same number, which may be only one (so "pair" is
//!     a slight misnomer), but will have no more than one individual and one groupwork

use core::fmt;
use std::collections::HashMap;
use std::iter::FilterMap;
use std::ops::Deref;

use anyhow::Result;
use futures::{stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::Assignment;
use gradescope_api::client::Client;
use gradescope_api::course::Course;
use gradescope_api::regrade::Regrade;
use gradescope_api::services::gs_service::GsService;
use gradescope_api::types::{GraderName, StudentName};
use serde::Serialize;

use self::pair::{HwPair, RegradeRefsPair, RegradesPair};

pub mod pair;

/// Finds pairs of individual and groupworks. For example, given
/// ```text
/// [ID1, ID3, ID4, GW1, Exam 1, GW2, GW4]
/// ```
/// we get back
/// ```text
/// [(1, ID1+GW1), (2, GW2), (3, ID3), (4, ID4+GW4)]
/// ```
pub fn find_homeworks(assignments: &[Assignment]) -> HashMap<HwNumber, HwPair> {
    let ids = Individual::get_from(assignments);
    let gws = Groupwork::get_from(assignments);
    HwPair::make_pairs(ids, gws)
}

pub async fn get_homework_regrades<'a>(
    homeworks: &HashMap<HwNumber<'a>, HwPair<'_>>,
    gradescope: &Client<impl GsService>,
    course: &Course,
) -> Result<HashMap<HwNumber<'a>, RegradesPair>> {
    stream::iter(homeworks)
        .then(|(num, pair)| async move {
            pair.as_deref()
                .map_same(|assignment| gradescope.get_regrades(course, assignment))
                .join_both()
                .await
                .try_both()
                .map(|x| (*num, x))
        })
        .try_collect()
        .await
}

pub fn group_regrades_by_grader<'map, 'num>(
    regrades: &'map HashMap<HwNumber<'num>, RegradesPair>,
) -> impl Iterator<Item = (HwNumber<'num>, &'map GraderName, RegradeRefsPair<'map>)> + 'map {
    regrades.iter().flat_map(|(num, pair)| {
        pair.as_ref()
            .group_by_same(Regrade::grader_name)
            .into_iter()
            .map(move |(grader, pair)| (*num, grader, pair))
    })
}

pub fn group_regrades_by_student<'map, 'num>(
    regrades: &'map HashMap<HwNumber<'num>, RegradesPair>,
) -> impl Iterator<Item = (HwNumber<'num>, &'map StudentName, RegradeRefsPair<'map>)> + 'map {
    regrades.iter().flat_map(|(num, pair)| {
        pair.as_ref()
            .group_by_same(Regrade::student_name)
            .into_iter()
            .map(move |(student, pair)| (*num, student, pair))
    })
}

/// A thing with an associated HW number
pub trait HasHwNumber<'a> {
    fn number(&self) -> HwNumber<'a>;
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct HwNumber<'a> {
    number: &'a str,
}

impl<'a> HwNumber<'a> {
    pub fn new(number: &'a str) -> Self {
        Self { number }
    }

    pub fn as_str(self) -> &'a str {
        self.number
    }
}

impl<'a> fmt::Display for HwNumber<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.number.fmt(f)
    }
}

type HwGetFromFn<'a, Slf> = fn(&'a Assignment) -> Option<Slf>;
type HwGetFromIter<'a, I, Slf> = FilterMap<I, HwGetFromFn<'a, Slf>>;

pub trait Homework<'a>:
    HasHwNumber<'a> + TryFrom<&'a Assignment, Error = ()> + Deref<Target = Assignment>
{
    fn to_pair(self) -> HwPair<'a>;

    fn get_from<I: IntoIterator<Item = &'a Assignment>>(
        assignments: I,
    ) -> HwGetFromIter<'a, I::IntoIter, Self> {
        let from_assignment = |assignment| Self::try_from(assignment).ok();

        assignments
            .into_iter()
            .filter_map(from_assignment as HwGetFromFn<'a, Self>)
    }

    fn numbered_pair(self) -> (HwNumber<'a>, HwPair<'a>) {
        (self.number(), self.to_pair())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Individual<'a> {
    number: HwNumber<'a>,
    assignment: &'a Assignment,
}

impl<'a> Homework<'a> for Individual<'a> {
    fn to_pair(self) -> HwPair<'a> {
        HwPair::from_individual(self)
    }
}

impl<'a> HasHwNumber<'a> for Individual<'a> {
    fn number(&self) -> HwNumber<'a> {
        self.number
    }
}

impl<'a> TryFrom<&'a Assignment> for Individual<'a> {
    type Error = ();

    fn try_from(assignment: &'a Assignment) -> Result<Self, Self::Error> {
        let number_text = assignment
            .name()
            .as_str()
            .strip_prefix("Homework ")
            .ok_or(())?;
        let number = HwNumber::new(number_text);
        Ok(Self { number, assignment })
    }
}

impl<'a> Deref for Individual<'a> {
    type Target = Assignment;

    fn deref(&self) -> &Self::Target {
        self.assignment
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Groupwork<'a> {
    number: HwNumber<'a>,
    assignment: &'a Assignment,
}

impl<'a> Homework<'a> for Groupwork<'a> {
    fn to_pair(self) -> HwPair<'a> {
        HwPair::from_groupwork(self)
    }
}

impl<'a> HasHwNumber<'a> for Groupwork<'a> {
    fn number(&self) -> HwNumber<'a> {
        self.number
    }
}

impl<'a> TryFrom<&'a Assignment> for Groupwork<'a> {
    type Error = ();

    fn try_from(assignment: &'a Assignment) -> Result<Self, Self::Error> {
        let number_text = assignment
            .name()
            .as_str()
            .strip_prefix("Groupwork ")
            .ok_or(())?;
        let number = HwNumber::new(number_text);
        Ok(Self { number, assignment })
    }
}

impl<'a> Deref for Groupwork<'a> {
    type Target = Assignment;

    fn deref(&self) -> &Self::Target {
        self.assignment
    }
}
