// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::PropertyGroup;
use crate::PropertyGroupComposed;
use crate::PropertyGroupDirect;
use crate::PropertyGroups;
use crate::error::IterError;
use crate::error::LookupError;

mod sealed {
    pub trait PropertyGroupsSealed {}

    impl PropertyGroupsSealed for crate::Instance<'_> {}
    impl PropertyGroupsSealed for crate::Service<'_> {}
    impl PropertyGroupsSealed for crate::Snapshot<'_> {}
}

/// Trait to look up direct-attached property groups from a [`Service`] or
/// [`Instance`].
///
/// Direct-attached property groups can be modified via
/// [`PropertyGroup::transaction()`].
///
/// [`Instance`]: crate::Instance
/// [`Service`]: crate::Service
pub trait HasDirectPropertyGroups: sealed::PropertyGroupsSealed {
    /// Look up a direct-attached property group by name.
    fn property_group_direct(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, PropertyGroupDirect>>, LookupError>;

    /// Iterate over all direct-attached property groups.
    fn property_groups_direct(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupDirect>, IterError>;
}

/// Trait to look up composed property groups from an [`Instance`] or
/// [`Snapshot`].
///
/// Composed property groups cannot be modified.
///
/// [`Instance`]: crate::Instance
/// [`Snapshot`]: crate::Snapshot
pub trait HasComposedPropertyGroups: sealed::PropertyGroupsSealed {
    /// Look up a composed property group by name.
    fn property_group_composed(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, PropertyGroupComposed>>, LookupError>;

    /// Iterate over all composed property groups.
    fn property_groups_composed(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupComposed>, IterError>;
}
