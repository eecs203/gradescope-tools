use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::ops::Deref;
use std::{iter, option};

use futures::future::{self, OptionFuture};
use gradescope_api::regrade::Regrade;
use itertools::Itertools;

use super::{Groupwork, HasHwNumber, HwNumber, Individual};

#[derive(Debug, Clone, Copy)]
pub struct Pair<Id, Gw> {
    // at least one of the two must be `Some(...)`
    id: Option<Id>,
    gw: Option<Gw>,
}

pub type SamePair<T> = Pair<T, T>;

impl<Id, Gw> Pair<Id, Gw> {
    pub fn from_individual(id: Id) -> Self {
        Self {
            id: Some(id),
            gw: None,
        }
    }

    pub fn from_groupwork(gw: Gw) -> Self {
        Self {
            id: None,
            gw: Some(gw),
        }
    }

    pub fn individual(&self) -> Option<&Id> {
        self.id.as_ref()
    }

    pub fn groupwork(&self) -> Option<&Gw> {
        self.gw.as_ref()
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        match (&self.id, &self.gw) {
            (Some(_), None) | (None, Some(_)) => 1,
            (Some(_), Some(_)) => 2,
            (None, None) => panic!("either id or gw should be `Some(...)`"),
        }
    }

    pub fn merge(self, other: Self) -> Self {
        Self {
            id: self.id.or(other.id),
            gw: self.gw.or(other.gw),
        }
    }

    pub fn as_ref(&self) -> Pair<&Id, &Gw> {
        Pair {
            id: self.id.as_ref(),
            gw: self.gw.as_ref(),
        }
    }

    pub fn as_deref(&self) -> Pair<&Id::Target, &Gw::Target>
    where
        Id: Deref,
        Gw: Deref,
    {
        Pair {
            id: self.id.as_deref(),
            gw: self.gw.as_deref(),
        }
    }

    pub fn map<T, U>(self, f: impl FnOnce(Id) -> T, g: impl FnOnce(Gw) -> U) -> Pair<T, U> {
        Pair {
            id: self.id.map(f),
            gw: self.gw.map(g),
        }
    }

    pub fn or<T>(self, f: impl FnOnce(Id) -> T, g: impl FnOnce(Gw) -> T) -> T {
        self.id
            .map(f)
            .or_else(|| self.gw.map(g))
            .expect("either id or gw should be `Some(...)`")
    }

    pub fn and(self, default_id: Id, default_gw: Gw) -> (Id, Gw) {
        (self.id.unwrap_or(default_id), self.gw.unwrap_or(default_gw))
    }

    pub async fn join_both<T, U>(self) -> Pair<T, U>
    where
        Id: Future<Output = T>,
        Gw: Future<Output = U>,
    {
        let (id, gw) = future::join(OptionFuture::from(self.id), OptionFuture::from(self.gw)).await;
        Pair { id, gw }
    }

    /// Pairs off groupworks and individuals with the same homework number.
    ///
    /// ## Example
    /// Given
    /// ```text
    /// [ID1, ID3, ID4]
    /// [GW1, GW2, GW4]
    /// ```
    /// we get back
    /// ```text
    /// [(1, ID1+GW1), (2, GW2), (3, ID3), (4, ID4+GW4)]
    /// ```
    pub fn make_pairs<'a>(
        ids: impl IntoIterator<Item = Id>,
        gws: impl IntoIterator<Item = Gw>,
    ) -> HashMap<HwNumber<'a>, Pair<Id, Gw>>
    where
        Id: HasHwNumber<'a>,
        Gw: HasHwNumber<'a>,
    {
        let ids = ids
            .into_iter()
            .map(|id| (id.number(), Pair::from_individual(id)));
        let gws = gws
            .into_iter()
            .map(|gw| (gw.number(), Pair::from_groupwork(gw)));
        let hws = ids.chain(gws);
        hws.into_grouping_map().fold_first(|a, _, b| a.merge(b))
    }
}

impl<T> SamePair<T> {
    pub fn map_same<U>(self, f: impl Fn(T) -> U) -> SamePair<U> {
        self.map(&f, &f)
    }
}

impl<T> IntoIterator for SamePair<T> {
    type Item = T;

    type IntoIter = iter::Chain<option::IntoIter<T>, option::IntoIter<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.gw.into_iter().chain(self.id)
    }
}

impl<Id, Gw, T, U> Pair<Id, Gw>
where
    Id: IntoIterator<Item = T>,
    Gw: IntoIterator<Item = U>,
{
    pub fn into_iter_both(self) -> (impl Iterator<Item = T>, impl Iterator<Item = U>) {
        (
            self.id.into_iter().flat_map(IntoIterator::into_iter),
            self.gw.into_iter().flat_map(IntoIterator::into_iter),
        )
    }

    pub fn into_iter_pairs(self) -> impl Iterator<Item = Pair<T, U>> {
        let (id, gw) = self.into_iter_both();
        let (id, gw) = (id.map(Pair::from_individual), gw.map(Pair::from_groupwork));
        id.chain(gw)
    }
}

impl<Id, Gw, T, U> Pair<Id, Gw>
where
    Id: IntoIterator<Item = T>,
    Gw: IntoIterator<Item = U>,
    T: Copy,
    U: Copy,
{
    pub fn group_by<K: Hash + Eq>(
        self,
        mut f: impl FnMut(T) -> K,
        mut g: impl FnMut(U) -> K,
    ) -> HashMap<K, Pair<Vec<T>, Vec<U>>> {
        self.into_iter_pairs()
            .map(|pair| (pair.or(&mut f, &mut g), pair.map(|x| vec![x], |x| vec![x])))
            .into_grouping_map()
            .fold_first(|acc, _, val| acc.vec_merge_both(val))
    }
}

impl<It, T> SamePair<It>
where
    It: IntoIterator<Item = T>,
    T: Copy,
{
    pub fn group_by_same<K: Hash + Eq>(self, f: impl Fn(T) -> K) -> HashMap<K, SamePair<Vec<T>>> {
        self.group_by(&f, &f)
    }
}

impl<Id, Gw, E> Pair<Result<Id, E>, Result<Gw, E>> {
    pub fn try_both(self) -> Result<Pair<Id, Gw>, E> {
        Ok(Pair {
            id: self.id.transpose()?,
            gw: self.gw.transpose()?,
        })
    }
}

impl<Id, Gw> Pair<Vec<Id>, Vec<Gw>> {
    pub fn vec_merge_both(self, other: Self) -> Self {
        Self {
            id: Self::merge_one(self.id, other.id),
            gw: Self::merge_one(self.gw, other.gw),
        }
    }

    fn merge_one<T>(a: Option<Vec<T>>, b: Option<Vec<T>>) -> Option<Vec<T>> {
        match (a, b) {
            (Some(mut a), Some(mut b)) => {
                a.append(&mut b);
                Some(a)
            }
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    }

    pub fn lens(&self) -> (usize, usize) {
        self.as_ref().map(Vec::len, Vec::len).and(0, 0)
    }
}

pub type HwPair<'a> = Pair<Individual<'a>, Groupwork<'a>>;

pub type RegradesPair = SamePair<Vec<Regrade>>;
pub type RegradeRefsPair<'a> = SamePair<Vec<&'a Regrade>>;
