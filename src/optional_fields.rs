//! A part of a struct whose fields are written into it all or none (`#[serde(flatten)]` over an optional part).

use std::ops::{Deref, DerefMut};

use serde::Serialize;

/// An optional part of a struct, its fields written into the struct (`#[serde(flatten)]`): all of them when the part is
/// there, none when it is not. The same JSON as a flattened `Option<T>`, which ts-rs (feature `ts`) cannot flatten. In
/// the TypeScript types the part's fields join the struct's own, each marked optional (`#[ts(optional)]`, or
/// `#[ts(as = "Option<..>", optional)]` for one that is not an Option): they are missing when the part is.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OptionalFields<T>(pub Option<T>);

impl<T> From<Option<T>> for OptionalFields<T> {
    fn from(part: Option<T>) -> Self {
        OptionalFields(part)
    }
}

impl<T> Deref for OptionalFields<T> {
    type Target = Option<T>;

    fn deref(&self) -> &Option<T> {
        &self.0
    }
}

impl<T> DerefMut for OptionalFields<T> {
    fn deref_mut(&mut self) -> &mut Option<T> {
        &mut self.0
    }
}

#[cfg(feature = "ts")]
impl<T: ts_rs::TS> ts_rs::TS for OptionalFields<T> {
    type WithoutGenerics = OptionalFields<ts_rs::Dummy>;
    type OptionInnerType = Self;

    fn name(cfg: &ts_rs::Config) -> String {
        T::name(cfg)
    }

    fn inline(cfg: &ts_rs::Config) -> String {
        T::inline(cfg)
    }

    fn inline_flattened(cfg: &ts_rs::Config) -> String {
        T::inline_flattened(cfg)
    }

    fn visit_dependencies(v: &mut impl ts_rs::TypeVisitor)
    where
        Self: 'static,
    {
        T::visit_dependencies(v);
    }

    fn visit_generics(v: &mut impl ts_rs::TypeVisitor)
    where
        Self: 'static,
    {
        T::visit_generics(v);
        v.visit::<T>();
    }
}
